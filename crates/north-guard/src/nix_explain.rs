use crate::guard::nix_activity::{Card, cards};
use std::process::Command;

const USAGE: &str = "Usage: north nix explain <nix subcommand | class>";

pub fn run(arguments: &[String]) -> Result<String, String> {
    match arguments {
        [verb, target] if verb == "explain" => explain(target, &local_help),
        _ => Err(USAGE.into()),
    }
}

fn local_help(subcommand: &str) -> Option<String> {
    let output = Command::new("nix").args([subcommand, "--help"]).env("MANPAGER", "cat").env("PAGER", "cat").env("TERM", "dumb").output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    (output.status.success() && !text.trim().is_empty()).then_some(text)
}

pub fn explain(target: &str, help: &dyn Fn(&str) -> Option<String>) -> Result<String, String> {
    let all = cards();
    let by_class = all.iter().find(|card| card.class.eq_ignore_ascii_case(target) || card.incident == target);
    let (card, subcommand) = match by_class {
        Some(card) => (card, card.explain.first().map(String::as_str)),
        None => match all.iter().find(|card| card.explain.iter().any(|name| name == target)) {
            Some(card) => (card, Some(target)),
            None => return Err(unknown(target, &all)),
        },
    };
    let mut out = card.render();
    if let Some(subcommand) = subcommand.filter(|_| !card.options.is_empty()) {
        out.push_str(&format!("\n\nnix {subcommand} --help on this host:\n"));
        match help(subcommand) {
            Some(text) => {
                for option in &card.options {
                    out.push('\n');
                    match section(&text, option) {
                        Some(found) => out.push_str(&found),
                        None => out.push_str(&format!("(no `{option}` section in this host's nix {subcommand} --help)")),
                    }
                    out.push('\n');
                }
            }
            None => out.push_str(&format!("\n(`nix {subcommand} --help` printed nothing on this host)\n")),
        }
    }
    Ok(out)
}

fn unknown(target: &str, all: &[Card]) -> String {
    let classes: Vec<&str> = all.iter().map(|card| card.class.as_str()).collect();
    let mut subcommands: Vec<&str> = all.iter().flat_map(|card| card.explain.iter().map(String::as_str)).collect();
    subcommands.sort_unstable();
    format!("north nix explain: no card for `{target}`\n{USAGE}\nclasses: {}\nsubcommands: {}", classes.join(" "), subcommands.join(" "))
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn section(text: &str, option: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|line| {
        let name = line.trim_start().trim_start_matches(['·', '•']).trim_start();
        name == option || name.strip_prefix(option).is_some_and(|rest| rest.starts_with(' '))
    })?;
    let depth = indent(lines[start]);
    let end = lines[start + 1..].iter().position(|line| !line.trim().is_empty() && indent(line) <= depth).map_or(lines.len(), |at| start + 1 + at);
    Some(lines[start..end].iter().map(|line| line.trim_end()).collect::<Vec<_>>().join("\n").trim_end().to_owned())
}

