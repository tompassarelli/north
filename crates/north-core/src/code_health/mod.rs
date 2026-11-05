mod habits;
#[cfg(test)]
mod habits_tests;
mod imports;
mod rust_modules;
mod rust_text;
mod source;
mod tick;
#[cfg(test)]
mod tick_tests;
#[cfg(test)]
mod tests;

use imports::imports_of;
use rust_modules::Crate;
use source::Source;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

const INPUT_LIMIT: u64 = 512 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Existence {
    Tree(Vec<String>),
    Root(String),
}

#[derive(Deserialize)]
struct SourceFile {
    path: String,
    text: String,
}

#[derive(Deserialize)]
struct Request {
    exists: Existence,
    #[serde(default)]
    manifests: Vec<SourceFile>,
    files: Vec<SourceFile>,
}

#[derive(Serialize)]
struct Response {
    imports: Vec<Vec<String>>,
}

pub fn run(arguments: &[String]) -> u8 {
    match arguments.first().map(String::as_str) {
        Some("imports") if arguments.len() == 1 => match imports_from_stdin() {
            Ok(()) => 0,
            Err(error) => fail(&error),
        },
        Some("habits") => habits_command(&arguments[1..]),
        Some("tick") => tick::run_tick(&arguments[1..]),
        _ => fail("usage: north code-health imports (JSON request on stdin, JSON response on stdout) | habits [--repo DIR] | tick [--dry-run] [--repos=a,b]"),
    }
}

fn fail(message: &str) -> u8 {
    eprintln!("north code-health: {message}");
    2
}

fn imports_from_stdin() -> Result<(), String> {
    let mut input = String::new();
    std::io::stdin()
        .take(INPUT_LIMIT)
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())?;
    let request: Request = serde_json::from_str(&input).map_err(|error| error.to_string())?;
    let response = answer(request);
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, &response).map_err(|error| error.to_string())?;
    writeln!(out).map_err(|error| error.to_string())
}

fn answer(request: Request) -> Response {
    let source = match request.exists {
        Existence::Tree(paths) => Source::tree(paths),
        Existence::Root(root) => Source::root(root.into()),
    };
    let manifests = request.manifests.iter().map(|m| (m.path.as_str(), m.text.as_str()));
    let crates = Crate::from_manifests(manifests);
    let imports = request
        .files
        .iter()
        .map(|file| imports_of(&file.path, &file.text, &source, &crates))
        .collect();
    Response { imports }
}

pub fn habits(root: &std::path::Path, edited: &str) -> Vec<String> {
    habits::findings(root, Some(edited)).unwrap_or_default()
}

fn habits_command(arguments: &[String]) -> u8 {
    let root = match arguments {
        [] => std::path::PathBuf::from("."),
        [flag, dir] if flag == "--repo" => std::path::PathBuf::from(dir),
        _ => return fail("usage: north code-health habits [--repo DIR]"),
    };
    match habits::findings(&root, None) {
        Ok(lines) if lines.is_empty() => 0,
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            1
        }
        Err(error) => fail(&error),
    }
}
