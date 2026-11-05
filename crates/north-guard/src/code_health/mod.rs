mod check;
mod cli;
#[cfg(test)]
mod cli_tests;
mod config;
mod crate_lines;
mod functions;
mod gate;
mod graph;
mod habits;
#[cfg(test)]
mod habits_tests;
mod history;
mod imports;
mod layers;
mod report;
mod resolver;
mod rust_modules;
mod rust_text;
mod signal;
mod snapshot;
mod source;
mod state;
mod tick;
#[cfg(test)]
mod tick_tests;
#[cfg(test)]
mod tests;

pub fn run(arguments: &[String]) -> u8 {
    match arguments.first().map(String::as_str) {
        Some("habits") => habits_command(&arguments[1..]),
        Some("tick") => tick::run_tick(&arguments[1..]),
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
