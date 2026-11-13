mod judge;
mod kinds;
mod turn;

use std::io::Read;

use north_base::platform::private_text::{self, ASKING_STATUS, AT_ONCE_ENV, Finding, render};

use crate::runtime_access::RuntimeAccess;

const USAGE: &str = "usage: north check privacy judge --text FILE|- [--kinds FILE]";

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
        Ok(None) => u8::try_from(ASKING_STATUS).unwrap_or(2),
        Ok(Some(findings)) if findings.is_empty() => 0,
        Ok(Some(findings)) => {
            print!("{}", render(&findings));
            1
        }
        Err(reason) => {
            eprintln!("privacy judge: {reason}");
            2
        }
    }
}

fn judged(text: &str, kinds: Option<&str>, access: &RuntimeAccess) -> Result<Option<Vec<Finding>>, String> {
    let text = match text {
        "-" => {
            let mut held = String::new();
            std::io::stdin().read_to_string(&mut held).map_err(|error| format!("cannot read the text: {error}"))?;
            held
        }
        file => std::fs::read_to_string(file).map_err(|error| format!("cannot read {file}: {error}"))?,
    };
    if text.trim().is_empty() {
        return Ok(Some(Vec::new()));
    }
    let list = kinds::load(kinds)?;
    let cache = judge::Cache::from_env();
    let at_once = std::env::var_os(AT_ONCE_ENV).is_some_and(|value| !value.is_empty());
    let Some(_turn) = cache.turn(&text, !at_once) else { return Ok(None) };
    let ran = std::sync::Mutex::new(std::collections::BTreeSet::new());
    let judged = judge::judge(&list, &text, &cache, &|tier, prompt| {
        let (answer, by) = judge::ask_subscription(tier, prompt, access)?;
        ran.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert((tier.name, by));
        Ok(answer)
    });
    let ran = ran.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !ran.is_empty() {
        let tiers = if ran.iter().any(|(tier, _)| *tier == "confirm") { "screen+confirm" } else { "screen" };
        let by: std::collections::BTreeSet<&str> = ran.iter().map(|(_, by)| by.as_str()).collect();
        private_text::write_receipt(&text, &format!("{tiers} by {}", by.into_iter().collect::<Vec<_>>().join(", ")));
    }
    judged.map(Some)
}
