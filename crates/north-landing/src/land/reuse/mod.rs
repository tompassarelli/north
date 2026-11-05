mod hash;
mod store;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

pub use hash::input_hash;
pub use store::{Key, Verdict, lookup, record};

const USAGE: &str = "usage: north land reuse hash [--repo DIR] --check LINE\n       north land reuse record --key LANE_HEAD:MAIN:INPUT --verdict pass|fail [--state DIR]\n       north land reuse lookup --key LANE_HEAD:MAIN:INPUT [--state DIR]";

pub fn state_dir() -> PathBuf {
    crate::runtime_floor_gate::state_dir().join("land-reuse")
}

fn option<'a>(arguments: &'a [String], name: &str) -> Option<&'a str> {
    arguments.windows(2).find(|pair| pair[0] == name).map(|pair| pair[1].as_str())
}

fn state(arguments: &[String]) -> PathBuf {
    option(arguments, "--state").map_or_else(state_dir, PathBuf::from)
}

fn key(arguments: &[String]) -> Option<Key> {
    option(arguments, "--key").and_then(Key::parse)
}

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((command, rest)) if command == "hash" => hash(rest),
        Some((command, rest)) if command == "record" => record_verdict(rest),
        Some((command, rest)) if command == "lookup" => lookup_verdict(rest),
        _ => {
            eprintln!("{USAGE}");
            64
        }
    }
}

fn hash(arguments: &[String]) -> u8 {
    let Some(line) = option(arguments, "--check") else {
        eprintln!("{USAGE}");
        return 64;
    };
    match input_hash(Path::new(option(arguments, "--repo").unwrap_or(".")), line) {
        Ok(digest) => {
            println!("{digest}");
            0
        }
        Err(error) => {
            eprintln!("land reuse hash: {error}");
            1
        }
    }
}

fn record_verdict(arguments: &[String]) -> u8 {
    let (Some(key), Some(verdict)) = (key(arguments), option(arguments, "--verdict").and_then(Verdict::parse)) else {
        eprintln!("{USAGE}");
        return 64;
    };
    match record(&state(arguments), &key, verdict) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("land reuse record: {error}");
            1
        }
    }
}

fn lookup_verdict(arguments: &[String]) -> u8 {
    let Some(key) = key(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    match lookup(&state(arguments), &key) {
        Some(verdict) => {
            println!("{}", verdict.word());
            0
        }
        None => 1,
    }
}
