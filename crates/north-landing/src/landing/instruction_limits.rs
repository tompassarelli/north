use std::collections::BTreeSet;
use std::path::Path;

use crate::platform::git;

const CATALOG: &str = "dotfiles/agents/catalog-config.json";
const LIMIT: usize = 100;

pub fn check(repository: &Path, reference: &str) -> u8 {
    match failures(repository, reference) {
        Err(error) => {
            eprintln!("agent-instruction-check: cannot check instructions: {error}");
            2
        }
        Ok((checked, failures)) if failures.is_empty() => {
            println!("agent-instruction-check: {checked} instruction files within limits");
            0
        }
        Ok((_, failures)) => {
            for failure in failures {
                eprintln!("agent-instruction-check: {failure}");
            }
            1
        }
    }
}

fn failures(repository: &Path, reference: &str) -> Result<(usize, Vec<String>), String> {
    let read = |args: &[&str]| {
        git::stdout(repository, args, |output| String::from_utf8_lossy(&output.stderr).trim_end().to_owned())
            .and_then(|bytes| String::from_utf8(bytes).map_err(|error| error.to_string()))
    };
    let snapshot = read(&["rev-parse", "--verify", &format!("{reference}^{{commit}}")])?.trim().to_owned();
    let listing = read(&["ls-tree", "-r", "--name-only", "-z", &snapshot])?;
    let paths: BTreeSet<&str> = listing.split('\0').filter(|path| !path.is_empty()).collect();
    let source = |path: &str| read(&["show", &format!("{snapshot}:{path}")]);
    let exempt = if paths.contains(CATALOG) { baselines(&source(CATALOG)?)? } else { BTreeSet::new() };
    let mut checked = 0;
    let mut failures = Vec::new();
    for path in paths.iter().filter(|path| Path::new(path).file_name().is_some_and(|name| name == "AGENTS.md")) {
        if exempt.contains(*path) {
            continue;
        }
        checked += 1;
        let lines = line_count(&source(path)?);
        if lines > LIMIT {
            failures.push(format!(
                "{path}@{snapshot}: {lines} lines, limit {LIMIT}; move detail to CLI --help, help TOPIC, or indexed docs and keep AGENTS.md as a router"
            ));
        }
    }
    Ok((checked, failures))
}

fn line_count(text: &str) -> usize {
    let breaks = ['\n', '\r', '\x0b', '\x0c', '\x1c', '\x1d', '\x1e', '\u{85}', '\u{2028}', '\u{2029}'];
    let mut count = 0;
    let mut characters = text.chars().peekable();
    let mut open = false;
    while let Some(character) = characters.next() {
        if breaks.contains(&character) {
            if character == '\r' && characters.peek() == Some(&'\n') {
                characters.next();
            }
            count += 1;
            open = false;
        } else {
            open = true;
        }
    }
    count + usize::from(open)
}

fn baselines(catalog: &str) -> Result<BTreeSet<String>, String> {
    let catalog: serde_json::Value = serde_json::from_str(catalog).map_err(|error| error.to_string())?;
    let mut exempt = BTreeSet::new();
    for entry in catalog.get("baselines").and_then(serde_json::Value::as_array).into_iter().flatten() {
        let owner = entry.get("owner").ok_or("baseline without owner")?;
        let field = |name: &str| owner.get(name).ok_or(format!("baseline owner without {name}"));
        if field("repo")?.as_str() == Some("nixos-config")
            && let Some(path) = field("path")?.as_str()
        {
            exempt.insert(path.to_owned());
        }
    }
    Ok(exempt)
}
