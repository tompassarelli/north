//! The list of variables a run carries across a boundary lives in `north_base::foundation::carry`, which the lander's reuse key reads too; this file is the `north capacity carry` command over it.

pub use north_base::foundation::carry::*;

const USAGE: &str = "usage: north capacity carry --list [--into sandbox|fetch|offload]";

/// `north capacity carry --list [--into BOUNDARY]` prints, one per line, the variables a run carries across BOUNDARY, or every carried variable.
pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let listed: Vec<&str> = match words.as_slice() {
        ["--list"] => every().collect(),
        ["--list", "--into", boundary] => match Boundary::parse(boundary) {
            Some(boundary) => names(boundary).collect(),
            None => return usage(),
        },
        _ => return usage(),
    };
    println!("{}", listed.join("\n"));
    0
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}
