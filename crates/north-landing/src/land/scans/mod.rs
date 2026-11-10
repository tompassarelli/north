pub mod boxes;
pub mod ci_jobs;

use std::path::{Path, PathBuf};

pub use north_land_scans::*;

pub fn run(arguments: &[String]) -> u8 {
    match arguments {
        [name, rest @ ..] if matches!(name.as_str(), "ci-budget" | "code-health" | "crate-lines" | "instruction-limits") => super::after::gates::scan(name, rest),
        [name] if name == "deps" => super::slots::deps::run(Path::new(".")),
        [name, range] if name == "boxes" => boxes::run(&PathBuf::from("."), range),
        [name, range] if name == "push-jobs" => ci_jobs::scan(&PathBuf::from("."), range, &mut std::io::stdout()),
        _ => north_land_scans::run(arguments),
    }
}
