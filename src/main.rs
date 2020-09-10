use std::path::{PathBuf, Path};

use handlebars::{Context, Decorator, Handlebars, Helper, HelperResult, Output, PathAndJson, RenderContext, RenderError};
use serde_json::map::Entry;
use serde_json::Value as JsonValue;
use structopt::StructOpt;
use toml::Value;
use anyhow::Context as _;

#[derive(StructOpt)]
#[structopt(name = "tt", about = "A simple templating program.")]
struct Arguments {
	#[structopt(parse(from_os_str))]
	template_file: PathBuf,

	#[structopt(parse(from_os_str))]
	values: PathBuf,

	#[structopt(conflicts_with("generate"))]
	section: Option<String>,

	#[structopt(long = "gen", parse(from_os_str), conflicts_with("section"))]
	generate: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
	let Arguments {
		template_file,
		values: config_file,
		section,
		generate,
	} = <_>::from_args();

	let (handlebars, template_name) = setup_handlebars(&template_file)?;

	let values = std::fs::read_to_string(&config_file)
		.with_context(|| format!("Unable to read configuration file: {}", config_file.display()))?;
	let mut value: Value = values.parse()
		.with_context(|| format!("Configuration file contains invalid toml: {}", config_file.display()))?;

	let value = if let Some(base) = generate {

		for (k, v) in value.as_table_mut().unwrap() {
			if let Some(table) = v.as_table_mut() {
				table.insert("name".into(), Value::String(k.into()));
			}

			let output = handlebars.render(&template_name, v)
				.context("Unable to render template")?;

			let mut target = base.join(&k);
			target.push(&template_name);
			println!("Generating {:?}", target);

			let parent = target.parent()
				.with_context(|| format!("Unable to locate parent: {}", target.display()))?;

			std::fs::create_dir_all(parent)
				.with_context(|| format!("Unable to create parent directories: {}", parent.display()))?;

			std::fs::write(&target, output)
				.with_context(|| format!("Unable to write output to target: {}", target.display()))?;
		}
		return Ok(());
	} else if let Some(section) = section {
		let value = value.get_mut(&section).unwrap();

		if let Some(table) = value.as_table_mut() {
			table.insert("name".into(), Value::String(section));
		}

		value
	} else {
		&mut value
	};

	let output = handlebars.render(&template_name, value)
		.with_context(|| format!("Unable to render template."))?;
	println!("{}", output);

	Ok(())
}

fn setup_handlebars(template: &Path) -> anyhow::Result<(Handlebars, String)> {
	let template_name = template.file_name()
		.with_context(|| format!("Unable to determine file name: {}", template.display()))?
		.to_string_lossy()
		.into_owned();

	let mut handlebars = handlebars::Handlebars::new();
	handlebars.register_template_file(&template_name, &template)
		.with_context(|| format!("Unable to read template file: {}", template.display()))?;

	handlebars.register_helper("upper", Box::new(upper));
	handlebars.register_decorator("includes", Box::new(includes));

	Ok((handlebars, template_name))
}

fn upper(h: &Helper, _: &Handlebars, _: &Context, _: &mut RenderContext, out: &mut dyn Output) -> HelperResult {
	// get parameter from helper or throw an error
	let param = h.param(0)
		.and_then(|v| v.value().as_str())
		.unwrap_or("");
	out.write(param.to_uppercase().as_ref())?;
	Ok(())
}

fn includes(
	d: &Decorator,
	_: &Handlebars,
	ctx: &Context,
	rc: &mut RenderContext,
) -> Result<(), RenderError> {
	let pointer = transform_path_to_pointer(rc.get_path());

	let t = d.param(0)
		.ok_or_else(|| RenderError::new("Parameter not provided"))?;

	let including = find_includes(t);
	if including.is_empty() {
		return Ok(());
	}

	let ctx_old = ctx;
	let mut ctx = ctx.clone();
	let data = ctx.data_mut();

	// The main object's data.
	let data = data.pointer_mut(&pointer)
		.ok_or_else(|| RenderError::new("Unable to locate data pointer. [It's possible that the library we're using change their internal format.]"))?;
	let data = data.as_object_mut()
		.ok_or_else(|| RenderError::new("Parameter is not pointing at an object. [The parameter that was passed to the decorator didn't point to an object]"))?;

	let includes = ctx_old.data().get("include")
		.ok_or_else(|| RenderError::new("Unable to locate include section."))?;

	for include in including {
		let included = includes.get(&include)
			.ok_or_else(|| RenderError::new(format!("Unable to locate \"{}\" within the include list.", include)))?;

		let included = included.as_object()
			.ok_or_else(|| RenderError::new(format!("Include \"{}\" value isn't a table.", include)))?;

		for (key, value) in included.iter() {
			let entry = data.entry(key.clone());

			insert_data(entry, value)
				.map_err(|err| RenderError::new(format!("{}", err)))?;
		}
	}

	rc.set_context(ctx);

	Ok(())
}

fn insert_data(entry: Entry, value: &JsonValue) -> anyhow::Result<()> {
	match entry {
		Entry::Occupied(mut entry) => {
			let existing_data = entry.get_mut().as_object_mut()
				.ok_or_else(|| anyhow::anyhow!("Existing entry isn't an object/table."))?;

			let data_to_insert = value.as_object()
				.ok_or_else(|| anyhow::anyhow!("Unable to insert included data. [It's not a object/table]"))?;

			for (key, value) in data_to_insert {
				existing_data.insert(key.clone(), value.clone());
			}

			return Ok(())
		}
		Entry::Vacant(entry) => {
			entry.insert(value.clone());
			return Ok(())
		}
	}
}

fn find_includes(pj: &PathAndJson) -> Vec<String> {
	let mut including = vec![];

	if let Some(value) = pj.value().get("include") {
		if let Some(values) = value.as_array() {
			for value in values {
				if let Some(value) = value.as_str() {
					including.push(value.to_string());
				}
			}
		}
	}

	including
}

fn transform_path_to_pointer(path: &str) -> String {
	let mut pointer = String::new();

	for level in path.split("/") {
		if level == "." {
			continue;
		}

		pointer.push('/');

		if level.starts_with("[") {
			pointer.push_str(&level[1..(level.len() - 1)]);
		} else {
			pointer.push_str(level);
		}
	}

	pointer
}