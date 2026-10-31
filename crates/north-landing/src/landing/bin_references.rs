use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: north bin-references --repo DIR --base REV --head REV [--nixos-config DIR] [--south DIR]";
const PREFIXES: [&str; 2] = ["tree/bin/", "${northBin}/"];
const SOURCE_REF: &str = "origin/main";

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Reference {
    pub entry: String,
    pub holder: String,
    pub location: String,
}

struct Source {
    label: &'static str,
    dir: PathBuf,
    pathspecs: &'static [&'static str],
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .output()
        .map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn deleted_entries(repo: &Path, base: &str, head: &str) -> Result<Vec<String>, String> {
    let listing = git(repo, &["diff", "--no-renames", "--name-only", "--diff-filter=D", base, head, "--", "bin"])?;
    Ok(listing.lines().filter_map(|path| path.strip_prefix("bin/")).map(str::to_owned).collect())
}

fn ends_name(rest: &str) -> bool {
    let mut chars = rest.chars();
    match chars.next() {
        None => true,
        Some('.') => !chars.next().is_some_and(char::is_alphanumeric),
        Some(next) => !(next.is_alphanumeric() || next == '_' || next == '-'),
    }
}

pub fn names_entry(line: &str, entry: &str) -> bool {
    PREFIXES.iter().any(|prefix| {
        let needle = format!("{prefix}{entry}");
        line.match_indices(&needle).any(|(at, _)| ends_name(&line[at + needle.len()..]))
    })
}

fn matching(entries: &[String], holder: &str, location: &str, line: &str) -> Vec<Reference> {
    entries
        .iter()
        .filter(|entry| names_entry(line, entry))
        .map(|entry| Reference { entry: entry.clone(), holder: holder.to_owned(), location: location.to_owned() })
        .collect()
}

fn source_references(source: &Source, entries: &[String]) -> Result<Vec<Reference>, String> {
    let main = source.dir.join("main");
    git(&main, &["fetch", "-q", "origin", "main"]).ok();
    let mut arguments = vec!["grep", "-n", "-I", "-F", "-e", PREFIXES[0], "-e", PREFIXES[1], SOURCE_REF, "--"];
    arguments.extend(source.pathspecs);
    let output = Command::new("git").arg("-C").arg(&main).args(&arguments).output().map_err(|error| format!("git grep: {error}"))?;
    match output.status.code() {
        Some(0) => {}
        Some(1) => return Ok(Vec::new()),
        _ => return Err(format!("git grep in {} failed: {}", main.display(), String::from_utf8_lossy(&output.stderr).trim())),
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let holder = format!("{} {SOURCE_REF}", source.label);
    let mut found = Vec::new();
    for hit in text.lines() {
        let Some(hit) = hit.strip_prefix(SOURCE_REF).and_then(|rest| rest.strip_prefix(':')) else { continue };
        let mut fields = hit.splitn(3, ':');
        let (Some(path), Some(number), Some(line)) = (fields.next(), fields.next(), fields.next()) else { continue };
        found.extend(matching(entries, &holder, &format!("{path}:{number}"), line));
    }
    Ok(found)
}

pub fn host_references(units: &str, entries: &[String]) -> Vec<Reference> {
    let mut unit = String::new();
    let mut found = Vec::new();
    for line in units.lines() {
        if let Some(path) = line.strip_prefix("# ") {
            unit = path.trim().to_owned();
            continue;
        }
        found.extend(matching(entries, "this host's systemctl --user cat", &unit, line));
    }
    found
}

fn host_units() -> Result<String, String> {
    let systemctl = |args: &[&str]| {
        Command::new("systemctl")
            .arg("--user")
            .args(args)
            .output()
            .map_err(|error| format!("systemctl: {error}"))
            .and_then(|output| {
                if output.status.success() {
                    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
                } else {
                    Err(format!("systemctl --user {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()))
                }
            })
    };
    let listing = systemctl(&["list-unit-files", "--no-legend", "--no-pager"])?;
    let units: Vec<&str> = listing
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| [".service", ".timer", ".path", ".socket"].iter().any(|suffix| name.ends_with(suffix)) && !name.ends_with("@.service"))
        .collect();
    if units.is_empty() {
        return Ok(String::new());
    }
    let mut arguments = vec!["cat", "--no-pager", "--"];
    arguments.extend(units);
    systemctl(&arguments)
}

pub fn refusal(found: &[Reference]) -> String {
    let mut lines: Vec<String> = found
        .iter()
        .map(|reference| {
            format!(
                "safe-push: ✗ bin/{} is deleted here, but {} still runs tree/bin/{} at {}",
                reference.entry, reference.holder, reference.entry, reference.location
            )
        })
        .collect();
    lines.push(
        "safe-push: switch that unit off the script first (every host runs the new unit before north-runtime-follow promotes the deletion), or keep bin/NAME as a shim until then; NOT pushed."
            .to_owned(),
    );
    lines.join("\n")
}

fn sources(nixos_config: PathBuf, south: PathBuf) -> [Source; 2] {
    [
        Source { label: "nixos-config", dir: nixos_config, pathspecs: &[".", ":(exclude)*.md"] },
        Source { label: "south", dir: south, pathspecs: &["agents/routines"] },
    ]
}

fn collect(repo: &Path, base: &str, head: &str, nixos_config: PathBuf, south: PathBuf) -> Result<(Vec<Reference>, Vec<String>), String> {
    let entries = deleted_entries(repo, base, head)?;
    if entries.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut found = Vec::new();
    let mut warnings = Vec::new();
    for source in sources(nixos_config, south) {
        if !source.dir.join("main").is_dir() {
            warnings.push(format!("safe-push: ⚠ no {} checkout at {}, so its units were not checked for bin/ entries this landing deletes.", source.label, source.dir.display()));
            continue;
        }
        found.extend(source_references(&source, &entries)?);
    }
    match host_units() {
        Ok(units) => found.extend(host_references(&units, &entries)),
        Err(error) => warnings.push(format!("safe-push: ⚠ cannot read this host's user units ({error}), so they were not checked for bin/ entries this landing deletes.")),
    }
    let distinct: BTreeSet<Reference> = found.into_iter().collect();
    Ok((distinct.into_iter().collect(), warnings))
}

fn home_dir(name: &str, variable: &str) -> PathBuf {
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code").join(name), PathBuf::from)
}

pub fn run(arguments: &[String]) -> u8 {
    if let [flag] = arguments
        && (flag == "-h" || flag == "--help")
    {
        println!("{USAGE}");
        return 0;
    }
    let mut repo = None;
    let mut base = None;
    let mut head = None;
    let mut nixos_config = home_dir("nixos-config", "SAFE_PUSH_NIXOS_CONFIG");
    let mut south = home_dir("south", "SAFE_PUSH_SOUTH");
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        let Some(value) = rest.next() else {
            eprintln!("{USAGE}");
            return 64;
        };
        match flag.as_str() {
            "--repo" => repo = Some(PathBuf::from(value)),
            "--base" => base = Some(value.clone()),
            "--head" => head = Some(value.clone()),
            "--nixos-config" => nixos_config = PathBuf::from(value),
            "--south" => south = PathBuf::from(value),
            _ => {
                eprintln!("{USAGE}");
                return 64;
            }
        }
    }
    let (Some(repo), Some(base), Some(head)) = (repo, base, head) else {
        eprintln!("{USAGE}");
        return 64;
    };
    match collect(&repo, &base, &head, nixos_config, south) {
        Ok((found, warnings)) => {
            warnings.iter().for_each(|warning| println!("{warning}"));
            if found.is_empty() {
                return 0;
            }
            println!("{}", refusal(&found));
            1
        }
        Err(error) => {
            println!("safe-push: ✗ cannot check whether a deployed unit still runs a bin/ entry this landing deletes: {error}; NOT pushed.");
            2
        }
    }
}

#[cfg(test)]
#[path = "bin_references_tests.rs"]
mod tests;
