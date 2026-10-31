use std::env;
use std::path::{Path, PathBuf};

use crate::error::NorthResult;

use super::status::{self, Options};
use super::{USAGE, replay, script};

pub struct Ports<'a> {
    pub harness: &'a Path,
    pub operator: &'a dyn Fn() -> NorthResult<PathBuf>,
    pub place: &'a dyn Fn(&[String]) -> u8,
}

pub fn run(arguments: &[String], ports: &Ports) -> u8 {
    match arguments.first().map(String::as_str) {
        Some("--score") if arguments.len() == 2 => match orchestration(ports) {
            Ok(config) => script::python_file(
                ports.harness,
                "project_score.py",
                [arguments[1].as_ref(), config.as_os_str()],
            ),
            Err(error) => {
                eprintln!("agents plan: {}", error.user_message());
                2
            }
        },
        Some("--score") => {
            eprintln!("{USAGE}");
            2
        }
        Some(verb @ ("--place" | "--credit")) => {
            let request = [
                vec!["place".to_owned(), verb.trim_start_matches("--").to_owned()],
                arguments[1..].to_vec(),
            ]
            .concat();
            (ports.place)(&request)
        }
        _ if arguments.iter().any(|argument| argument == "--quote") => {
            script::run(ports.harness, "project_plan.py", arguments)
        }
        _ => {
            status::update_status();
            run_status(arguments, ports)
        }
    }
}

fn run_status(arguments: &[String], ports: &Ports) -> u8 {
    match Options::parse(arguments) {
        Some(Options { at: Some(scenario), json, .. }) => replay::run(&scenario, json),
        Some(options) => match orchestration(ports) {
            Ok(config) => status::run(&options, ports.harness, &config),
            Err(error) => {
                eprintln!("agents plan: {}", error.user_message());
                2
            }
        },
        None => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn orchestration(ports: &Ports) -> NorthResult<PathBuf> {
    match env::var_os("AGENTS_ORCHESTRATION") {
        Some(path) => Ok(PathBuf::from(path)),
        None => (ports.operator)().map(|directory| directory.join("orchestration.toml")),
    }
}
