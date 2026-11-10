mod ambient;
mod budget;
mod change_cost;
mod check;
mod comments;
#[cfg(test)]
mod comments_tests;
mod cli;
#[cfg(test)]
mod cli_tests;
mod config;
mod coupling;
mod crate_lines;
mod finding;
mod fix;
pub use fix::Suggestion;
mod functions;
mod gate;
#[cfg(test)]
mod gate_tests;
mod graph;
mod habits;
#[cfg(test)]
mod habits_tests;
mod history;
mod implicit;
mod imports;
#[cfg(test)]
mod imports_tests;
mod layers;
pub use layers::{Placement, placement};
mod report;
mod resolver;
mod rust_modules;
mod rust_text;
mod signal;
mod snapshot;
mod source;
mod state;

const HABITS_USAGE: &str = "usage: north code-health habits [--repo DIR] [--base REV [--head REV] [--shadow]] [--new-since REV] [--grown-since REV]";

pub fn run(arguments: &[String]) -> u8 {
    match arguments.first().map(String::as_str) {
        Some("habits") => habits_command(&arguments[1..]),
        Some("crate-lines") => crate_lines::run(&arguments[1..]),
        Some("layers") => layers::run(&arguments[1..]),
        None => crate_lines::run(&[]),
        _ => cli::run(arguments),
    }
}

fn fail(message: &str) -> u8 {
    eprintln!("north code-health: {message}");
    2
}

pub fn habit_warnings(root: &std::path::Path, edited: &str) -> Vec<String> {
    habits::findings_without_fix(root, edited).unwrap_or_default()
}

pub fn habit_suggestions(root: &std::path::Path, edited: &str) -> Vec<Suggestion> {
    habits::suggestions(root, edited).unwrap_or_default()
}

pub fn crate_headroom(root: &std::path::Path, edited: &str) -> Option<String> {
    crate_lines::headroom(root, edited)
}

fn habits_command(arguments: &[String]) -> u8 {
    let mut root = std::path::PathBuf::from(".");
    let (mut base, mut head, mut shadow, mut grown_since, mut new_since) = (None, "HEAD".to_owned(), false, None, None);
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let mut value = || rest.next().cloned();
        match argument.as_str() {
            "--repo" => root = value().map(Into::into).unwrap_or_default(),
            "--base" => base = value(),
            "--head" => head = value().unwrap_or_default(),
            "--shadow" => shadow = true,
            "--new-since" => new_since = value(),
            "--grown-since" => grown_since = value(),
            _ => return fail(HABITS_USAGE),
        }
    }
    let mut found = match &new_since {
        Some(rev) => habits::findings_new(&root, rev),
        None => match &base {
            Some(base) => comments::added(&root, base, &head),
            None => habits::findings(&root, None),
        },
    };
    if let Some(rev) = &grown_since {
        found = found.and_then(|mut lines| {
            lines.extend(habits::grown(&root, rev)?);
            Ok(lines)
        });
    }
    match found {
        Ok(lines) if lines.is_empty() => 0,
        Ok(lines) => {
            for line in lines {
                println!("{}{line}", if shadow { "would refuse: " } else { "" });
            }
            u8::from(!shadow)
        }
        Err(error) => fail(&error),
    }
}
