use std::path::Path;

use toml_edit::DocumentMut;

const HELP: &str = "src/cli/help.txt";
const CEILING_KEY: &str = "help_visible_ceiling";
const SUMMARY_LIMIT: usize = 70;

struct Entry<'a> {
    name: &'a str,
    summary: &'a str,
}

fn entries(help: &str) -> Vec<Entry<'_>> {
    help.lines()
        .filter_map(|line| line.strip_prefix("  ").filter(|rest| rest.starts_with(|first: char| first.is_ascii_alphanumeric())))
        .map(|line| {
            let (name, summary) = line.split_once("  ").unwrap_or((line, ""));
            Entry { name: name.trim(), summary: summary.trim() }
        })
        .collect()
}

fn ceiling(defaults: &str) -> Result<usize, String> {
    let document: DocumentMut = defaults.parse().map_err(|error| format!("harness/defaults.toml does not parse: {error}"))?;
    document
        .get(CEILING_KEY)
        .and_then(|table| table.get("value"))
        .and_then(|value| value.as_integer())
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| format!("harness/defaults.toml has no whole number for {CEILING_KEY}"))
}

fn read(north: &Path, relative: &str) -> Result<String, String> {
    let path = north.join(relative);
    std::fs::read_to_string(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

pub(super) fn help_surface(north: &Path) -> (String, bool) {
    let (help, limit) = match read(north, HELP).and_then(|help| Ok((help, ceiling(&read(north, "harness/defaults.toml")?)?))) {
        Ok(found) => found,
        Err(error) => return (error, false),
    };
    let listed = entries(&help);
    let mut problems: Vec<String> = listed
        .iter()
        .filter_map(|entry| {
            let length = entry.summary.chars().count();
            if entry.summary.contains('|') {
                Some(format!("{HELP}: the {} summary holds '|'; list its subcommands in north {} --help", entry.name, entry.name))
            } else if length > SUMMARY_LIMIT {
                Some(format!("{HELP}: the {} summary is {length} characters, over {SUMMARY_LIMIT}", entry.name))
            } else {
                None
            }
        })
        .collect();
    if listed.len() > limit {
        problems.push(format!("{HELP}: {} visible commands, over the {limit} in harness/defaults.toml {CEILING_KEY}; fold a family under its domain noun", listed.len()));
    }
    if problems.is_empty() {
        (format!("north --help: {} visible commands of {limit}, every summary at most {SUMMARY_LIMIT} characters with no '|'", listed.len()), true)
    } else {
        (problems.join("\n"), false)
    }
}
