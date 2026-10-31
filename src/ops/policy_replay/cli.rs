use std::path::PathBuf;

use super::point::{PromoteArgs, promote};
use crate::error::NorthError;

const USAGE: &str = "usage: north policy-replay promote SCENARIO.json... [--candidate PATH] [--operating-point PATH] [--accept-drop REASON]";

struct Options {
    scenarios: Vec<PathBuf>,
    candidate: PathBuf,
    operating_point: Option<PathBuf>,
    accept_drop: Option<String>,
}

fn default_candidate() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".local/state/agents/replay/operating-point-candidate.toml")
}

fn parse(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options { scenarios: Vec::new(), candidate: default_candidate(), operating_point: None, accept_drop: None };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let mut value = |flag: &str| rest.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        match argument.as_str() {
            "--candidate" => options.candidate = PathBuf::from(value("--candidate")?),
            "--operating-point" => options.operating_point = Some(PathBuf::from(value("--operating-point")?)),
            "--accept-drop" => options.accept_drop = Some(value("--accept-drop")?),
            flag if flag.starts_with("--") => return Err(format!("unrecognized arguments: {flag}")),
            file => options.scenarios.push(PathBuf::from(file)),
        }
    }
    if options.scenarios.is_empty() {
        return Err("the following arguments are required: scenarios".into());
    }
    Ok(options)
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let Some((verb, rest)) = arguments.split_first().filter(|(verb, _)| verb.as_str() == "promote") else {
        eprintln!("{USAGE}");
        return 2;
    };
    debug_assert_eq!(verb, "promote");
    let options = match parse(rest) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{USAGE}\nnorth policy-replay: error: {message}");
            return 2;
        }
    };
    let args = PromoteArgs {
        candidate: &options.candidate,
        scenarios: &options.scenarios,
        operating_point: options.operating_point.as_deref(),
        accept_drop: options.accept_drop.as_deref(),
    };
    match promote(&args) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(NorthError::Refused(message)) => {
            eprintln!("{message}");
            1
        }
        Err(error) => {
            eprintln!("policy-replay: {error}");
            1
        }
    }
}
