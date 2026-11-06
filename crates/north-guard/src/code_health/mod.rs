mod budget;
mod change_cost;
mod check;
mod cli;
#[cfg(test)]
mod cli_tests;
mod config;
mod coupling;
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
mod test_determinism;
mod tick;
#[cfg(test)]
mod tick_tests;

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
    habits::findings(root, Some(edited)).unwrap_or_default().iter().map(habits::Finding::render).collect()
}

fn habits_command(arguments: &[String]) -> u8 {
    match arguments {
        [] => habits_in(std::path::Path::new(".")),
        [flag, dir] if flag == "--repo" => habits_in(std::path::Path::new(dir)),
        [flag, dir, since, base] if flag == "--repo" && since == "--grown-since" => grown_in(std::path::Path::new(dir), base),
        _ => fail("usage: north code-health habits [--repo DIR [--grown-since BASE]]"),
    }
}

fn habits_in(root: &std::path::Path) -> u8 {
    match habits::findings(root, None) {
        Ok(found) => {
            for finding in &found {
                println!("{}", finding.render());
            }
            u8::from(found.iter().any(|finding| !finding.advisory))
        }
        Err(error) => fail(&error),
    }
}

fn grown_in(root: &std::path::Path, base: &str) -> u8 {
    match test_determinism::grown(root, base) {
        Ok(lines) => {
            for line in &lines {
                println!("{line}");
            }
            u8::from(!lines.is_empty())
        }
        Err(error) => fail(&error),
    }
}
