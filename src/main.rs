use std::path::PathBuf;

use structopt::StructOpt;
use toml::Value;

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

const NAME: &str = "input";

fn main() {
	let Arguments {
		template_file,
		values,
		section,
		generate,
	} = <_>::from_args();

	let values = std::fs::read_to_string(&values).unwrap();
	let mut value: Value = values.parse().unwrap();

	let mut handlebars = handlebars::Handlebars::new();
	handlebars.register_template_file(NAME, &template_file).unwrap();


	let value = if let Some(base) = generate {
		let name: String = template_file.file_name()
			.unwrap()
			.to_string_lossy()
			.into_owned();

		for (k, v) in value.as_table_mut().unwrap() {
			if let Some(table) = v.as_table_mut() {
				table.insert("name".into(), Value::String(k.into()));
			}

			let output = handlebars.render(NAME, v).unwrap();

			let mut target = base.join(&k);
			target.push(&name);
			println!("Generating {:?}", target);

			std::fs::create_dir_all(target.parent().unwrap()).unwrap();
			std::fs::write(&target, output).unwrap();
		}
		return;
	} else if let Some(section) = section {
		let value = value.get_mut(&section).unwrap();

		if let Some(table) = value.as_table_mut() {
			table.insert("name".into(), Value::String(section));
		}

		value
	} else {
		&mut value
	};

	let output = handlebars.render(NAME, value).unwrap();
	println!("{}", output);
}
