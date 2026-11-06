mod judge;
mod kinds;

use std::io::Read;

use north_base::platform::private_text::{Finding, render};

use crate::runtime_access::RuntimeAccess;

const USAGE: &str = "usage: north privacy judge --text FILE|- [--kinds FILE]";

pub fn run(arguments: &[String], access: &RuntimeAccess) -> u8 {
    let Some(("judge", options)) = arguments.split_first().map(|(verb, rest)| (verb.as_str(), rest)) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let (mut text, mut kinds) = (None, None);
    let mut rest = options.iter();
    while let Some(option) = rest.next() {
        match (option.as_str(), rest.next()) {
            ("--text", Some(value)) => text = Some(value),
            ("--kinds", Some(value)) => kinds = Some(value),
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    let Some(text) = text else {
        eprintln!("{USAGE}");
        return 2;
    };
    match judged(text, kinds.map(String::as_str), access) {
        Ok(findings) if findings.is_empty() => 0,
        Ok(findings) => {
            print!("{}", render(&findings));
            1
        }
        Err(reason) => {
            eprintln!("privacy judge: {reason}");
            2
        }
    }
}

fn judged(text: &str, kinds: Option<&str>, access: &RuntimeAccess) -> Result<Vec<Finding>, String> {
    let text = match text {
        "-" => {
            let mut held = String::new();
            std::io::stdin().read_to_string(&mut held).map_err(|error| format!("cannot read the text: {error}"))?;
            held
        }
        file => std::fs::read_to_string(file).map_err(|error| format!("cannot read {file}: {error}"))?,
    };
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let list = kinds::load(kinds)?;
    judge::judge(&list, &text, &judge::Cache::from_env(), &mut |tier, prompt| judge::ask_subscription(tier, prompt, access))
}
