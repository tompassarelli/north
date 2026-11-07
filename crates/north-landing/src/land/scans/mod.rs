pub mod boxes;

use std::path::{Path, PathBuf};

pub use north_land_scans::*;

pub fn run(arguments: &[String]) -> u8 {
    match arguments {
        [name, rest @ ..] if matches!(name.as_str(), "ci-budget" | "code-health" | "crate-lines" | "instruction-limits") => super::after::gates::scan(name, rest),
        [name] if name == "deps" => super::slots::deps::run(Path::new(".")),
        [name, range] if name == "boxes" => boxes::run(&PathBuf::from("."), range),
        _ => north_land_scans::run(arguments),
    }
}
