use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

const USAGE: &str = "usage: north merge-driver set [--config FILE] BASE OURS THEIRS PATH | continue [--config FILE]";
pub struct Driver {
    pub name: &'static str,
    pub description: &'static str,
    pub arguments: &'static str,
}

pub const DRIVERS: &[Driver] = &[Driver { name: "set", description: "order-insensitive unique-entry lists and regenerated files (harness/merge-sets.toml)", arguments: "merge-driver set %O %A %B %P" }];
const GIT_DRIVERS: [&str; 3] = ["text", "binary", "union"];
const KINDS: [&str; 4] = ["lines", "block", "toml-array", "regenerate"];
static PROGRAM: OnceLock<PathBuf> = OnceLock::new();
static REPORTED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
pub const DEFAULT_CONFIG: &str = "harness/merge-sets.toml";
const PLACEHOLDER: char = '\u{1}';

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub key: String,
    pub text: String,
}

#[derive(Debug, Eq, PartialEq)]
enum Change {
    Add(String),
    Modify(String),
    Remove,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Identity {
    Line,
    FirstWord,
}

fn first_word(entry: &Entry) -> &str {
    entry.text.split_whitespace().next().unwrap_or_default()
}

fn doubled_first_words(entries: &[Entry]) -> BTreeSet<&str> {
    let mut seen = BTreeSet::new();
    entries.iter().map(first_word).filter(|word| !seen.insert(*word)).collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Lines(Identity),
    Block { start: String, end: String },
    TomlArray(Vec<String>),
    Regenerate { command: String, sources: Vec<String>, generator: Vec<String> },
}

#[derive(Debug)]
pub(crate) struct Rule {
    pub(crate) path: String,
    pub(crate) kind: Kind,
}

pub fn run_drivers_with(program: &Path) {
    let _ = PROGRAM.set(program.to_owned());
}

fn driver_program() -> Option<PathBuf> {
    PROGRAM.get().cloned().or_else(|| std::env::current_exe().ok().filter(|exe| exe.file_name().is_some_and(|name| name == "north")))
}

fn shell_quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

pub fn driver_arguments() -> Vec<String> {
    let Some(program) = driver_program() else { return Vec::new() };
    let program = shell_quoted(&program);
    DRIVERS
        .iter()
        .flat_map(|driver| ["-c".to_owned(), format!("merge.{}.name={}", driver.name, driver.description), "-c".to_owned(), format!("merge.{}.driver={program} {}", driver.name, driver.arguments)])
        .collect()
}

pub fn undriven_attributes(attributes: &str) -> Vec<String> {
    attributes
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .flat_map(|line| line.split_whitespace().skip(1))
        .filter_map(|attribute| attribute.strip_prefix("merge="))
        .filter(|name| !GIT_DRIVERS.contains(name) && !DRIVERS.iter().any(|driver| driver.name == *name))
        .map(str::to_owned)
        .collect()
}

pub fn line_entry(line: &str) -> Entry {
    let key = line.split_once(" #").map_or(line, |(key, _)| key).trim().to_owned();
    Entry { key, text: line.to_owned() }
}

fn index(side: &[Entry], label: &str) -> Result<BTreeMap<String, String>, String> {
    let mut seen = BTreeMap::new();
    for entry in side {
        if seen.insert(entry.key.clone(), entry.text.clone()).is_some() {
            return Err(format!("duplicate entry {:?} in {label}", entry.key));
        }
    }
    Ok(seen)
}

fn changes(base: &BTreeMap<String, String>, side: &BTreeMap<String, String>) -> BTreeMap<String, Change> {
    let mut out = BTreeMap::new();
    for (key, text) in side {
        match base.get(key) {
            None => drop(out.insert(key.clone(), Change::Add(text.clone()))),
            Some(old) if old != text => drop(out.insert(key.clone(), Change::Modify(text.clone()))),
            Some(_) => {}
        }
    }
    for key in base.keys().filter(|key| !side.contains_key(*key)) {
        out.insert(key.clone(), Change::Remove);
    }
    out
}

fn is_sorted(side: &[Entry]) -> bool {
    side.windows(2).all(|pair| pair[0].text <= pair[1].text)
}

pub fn merge_entries(base: &[Entry], ours: &[Entry], theirs: &[Entry]) -> Result<Vec<Entry>, String> {
    let base_index = index(base, "the base")?;
    let ours_changes = changes(&base_index, &index(ours, "ours")?);
    let theirs_changes = changes(&base_index, &index(theirs, "theirs")?);
    let mut outcome: BTreeMap<&String, &Change> = BTreeMap::new();
    for (key, change) in &ours_changes {
        match theirs_changes.get(key) {
            None => drop(outcome.insert(key, change)),
            Some(Change::Remove) if *change == Change::Remove => drop(outcome.insert(key, change)),
            Some(Change::Modify(text)) if *change == Change::Modify(text.clone()) => drop(outcome.insert(key, change)),
            Some(Change::Add(_)) if matches!(change, Change::Add(_)) => return Err(format!("both sides add {key:?}: a duplicate entry")),
            Some(_) => return Err(format!("the sides disagree on {key:?}: one removes or edits an entry the other removes or edits differently")),
        }
    }
    for (key, change) in &theirs_changes {
        if !ours_changes.contains_key(key) {
            outcome.insert(key, change);
        }
    }
    let mut merged = Vec::new();
    for entry in base {
        match outcome.get(&entry.key) {
            None => merged.push(entry.clone()),
            Some(Change::Modify(text)) => merged.push(Entry { key: entry.key.clone(), text: text.clone() }),
            Some(_) => {}
        }
    }
    let mut added = outcome
        .iter()
        .filter_map(|(key, change)| match change {
            Change::Add(text) => Some(Entry { key: (*key).clone(), text: text.clone() }),
            _ => None,
        })
        .collect::<Vec<_>>();
    added.sort_by(|a, b| a.text.cmp(&b.text));
    merged.extend(added);
    if is_sorted(ours) && is_sorted(theirs) {
        merged.sort_by(|a, b| a.text.cmp(&b.text));
    }
    Ok(merged)
}

struct LinesFile {
    header: Vec<String>,
    entries: Vec<Entry>,
}

fn parse_lines(text: &str) -> Result<LinesFile, String> {
    let mut header = Vec::new();
    let mut entries = Vec::new();
    for line in text.lines() {
        let plain = line.trim().is_empty() || line.trim_start().starts_with('#');
        if plain && entries.is_empty() {
            header.push(line.to_owned());
        } else if plain {
            return Err("a comment or blank line follows the first entry".to_owned());
        } else {
            entries.push(line_entry(line));
        }
    }
    Ok(LinesFile { header, entries })
}

fn merge_lines(base: &str, ours: &str, theirs: &str, identity: Identity) -> Result<String, String> {
    let (base, ours, theirs) = (parse_lines(base)?, parse_lines(ours)?, parse_lines(theirs)?);
    let header = if ours.header == base.header {
        theirs.header
    } else if theirs.header == base.header || theirs.header == ours.header {
        ours.header
    } else {
        return Err("both sides edit the leading comments".to_owned());
    };
    let entries = merge_entries(&base.entries, &ours.entries, &theirs.entries)?;
    if identity == Identity::FirstWord {
        let inherited = doubled_first_words(&ours.entries);
        if let Some(word) = doubled_first_words(&entries).into_iter().find(|word| !inherited.contains(word)) {
            return Err(format!("two lines for {word:?}: both sides map it differently"));
        }
    }
    let mut out = String::new();
    for line in header.iter().chain(entries.iter().map(|entry| &entry.text)) {
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

struct ArraySet {
    key: String,
    line: String,
    entries: Vec<Entry>,
}

struct ArrayFile {
    skeleton: String,
    sets: BTreeMap<String, ArraySet>,
}

fn table_name(section: &[&str]) -> String {
    section
        .iter()
        .find_map(|line| line.trim().strip_prefix("name").and_then(|rest| rest.trim_start().strip_prefix('=')))
        .map_or_else(String::new, |value| value.trim().to_owned())
}

fn parse_array(line: &str, key: &str) -> Option<Vec<Entry>> {
    let rest = line.trim_start().strip_prefix(key)?.trim_start().strip_prefix('=')?.trim();
    let inner = rest.strip_prefix('[')?.strip_suffix(']')?.trim();
    if inner.is_empty() {
        return Some(Vec::new());
    }
    let mut entries = Vec::new();
    for part in inner.trim_end_matches(',').split(',') {
        let value = part.trim().strip_prefix('"')?.strip_suffix('"')?;
        if value.contains(['"', '\\']) {
            return None;
        }
        entries.push(Entry { key: value.to_owned(), text: value.to_owned() });
    }
    Some(entries)
}

fn parse_array_file(text: &str, keys: &[String]) -> Result<ArrayFile, String> {
    let mut skeleton = String::new();
    let mut sets = BTreeMap::new();
    let lines = text.lines().collect::<Vec<_>>();
    let mut start = 0;
    while start < lines.len() {
        let mut end = start + 1;
        while end < lines.len() && !lines[end].starts_with('[') {
            end += 1;
        }
        let section = &lines[start..end];
        let table = format!("{}{}", section[0], table_name(section));
        for line in section {
            let key = keys.iter().find(|key| line.trim_start().strip_prefix(key.as_str()).is_some_and(|rest| rest.trim_start().starts_with('=')));
            if let Some(key) = key {
                let id = format!("{table} {key}");
                let entries = parse_array(line, key).ok_or_else(|| format!("{key} in {} is not a one-line array of plain strings", section[0]))?;
                if sets.insert(id.clone(), ArraySet { key: key.clone(), line: (*line).to_owned(), entries }).is_some() {
                    return Err(format!("{key} appears twice in {}", section[0]));
                }
                skeleton.push(PLACEHOLDER);
                skeleton.push_str(&id);
            } else {
                skeleton.push_str(line);
            }
            skeleton.push('\n');
        }
        start = end;
    }
    Ok(ArrayFile { skeleton, sets })
}

fn render_array(key: &str, entries: &[Entry], template: &str) -> String {
    let indent = &template[..template.len() - template.trim_start().len()];
    let quoted = entries.iter().map(|entry| format!("\"{}\"", entry.text)).collect::<Vec<_>>().join(", ");
    format!("{indent}{key} = [{quoted}]")
}

fn text_merge(base: &str, ours: &str, theirs: &str) -> Result<(String, bool), String> {
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let paths = ["ours", "base", "theirs"].map(|name| dir.path().join(name));
    for (path, text) in paths.iter().zip([ours, base, theirs]) {
        fs::write(path, text).map_err(|error| error.to_string())?;
    }
    let status = Command::new("git")
        .args(["merge-file", "-L", "ours", "-L", "base", "-L", "theirs"])
        .args(&paths)
        .status()
        .map_err(|error| format!("git merge-file: {error}"))?;
    let code = status.code().ok_or("git merge-file was killed")?;
    if !(0..=127).contains(&code) {
        return Err(format!("git merge-file failed with {code}"));
    }
    Ok((fs::read_to_string(&paths[0]).map_err(|error| error.to_string())?, code > 0))
}

#[cfg(test)]
fn merge_arrays(base: &str, ours: &str, theirs: &str, key: &str) -> Result<String, String> {
    merge_array_keys(base, ours, theirs, &[key.to_owned()])
}

fn merge_array_keys(base: &str, ours: &str, theirs: &str, keys: &[String]) -> Result<String, String> {
    let (base, ours, theirs) = (parse_array_file(base, keys)?, parse_array_file(ours, keys)?, parse_array_file(theirs, keys)?);
    let (merged, conflicts) = text_merge(&base.skeleton, &ours.skeleton, &theirs.skeleton)?;
    if conflicts {
        return Err("the text around the arrays conflicts".to_owned());
    }
    let present = merged.lines().filter_map(|line| line.strip_prefix(PLACEHOLDER)).collect::<BTreeSet<_>>();
    for side in [&ours, &theirs] {
        for (id, set) in &side.sets {
            let changed = base.sets.get(id).is_none_or(|old| old.entries != set.entries);
            if changed && !present.contains(id.as_str()) {
                return Err(format!("one side drops {id:?} while the other changes it"));
            }
        }
    }
    let empty = ArraySet { key: String::new(), line: String::new(), entries: Vec::new() };
    let mut out = String::new();
    for line in merged.lines() {
        let Some(id) = line.strip_prefix(PLACEHOLDER) else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        let (b, o, t) = (base.sets.get(id).unwrap_or(&empty), ours.sets.get(id).unwrap_or(&empty), theirs.sets.get(id).unwrap_or(&empty));
        let entries = merge_entries(&b.entries, &o.entries, &t.entries).map_err(|error| format!("{id}: {error}"))?;
        let template = [o, t, b].into_iter().find(|set| !set.line.is_empty()).unwrap_or(&empty);
        let line = [o, t, b]
            .into_iter()
            .find(|set| set.entries == entries && !set.line.is_empty())
            .map_or_else(|| render_array(&template.key, &entries, &template.line), |set| set.line.clone());
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

fn string_array(table: &toml_edit::Table, name: &str) -> Option<Vec<String>> {
    let array = table.get(name)?.as_array()?;
    let sources = array.iter().map(|value| value.as_str().map(str::to_owned)).collect::<Option<Vec<_>>>()?;
    Some(sources).filter(|sources| !sources.is_empty())
}

fn report_unknown(kind: &str, config: &Path) {
    let first = REPORTED.lock().map_or(true, |mut reported| reported.insert(kind.to_owned()));
    if first {
        eprintln!("north merge-driver: {}: skipping the entries of kind {kind:?}, which this north does not know; git merges their files as text", config.display());
    }
}

fn touches(paths: &[String], prefixes: &[String]) -> bool {
    paths.iter().any(|path| prefixes.iter().any(|prefix| path.starts_with(prefix.as_str())))
}

pub(crate) fn generator_moved(rules: &[Rule], ahead: &[String], pick: &[String]) -> Option<String> {
    rules.iter().find_map(|rule| {
        let Kind::Regenerate { sources, generator, .. } = &rule.kind else { return None };
        let regenerated = pick.contains(&rule.path) || touches(pick, sources);
        (touches(ahead, generator) && regenerated).then(|| format!("a lane ahead of it in this candidate changes the generator of {}, which this running lander cannot regenerate with", rule.path))
    })
}

pub(crate) fn read_rules(config: &Path) -> Result<Vec<Rule>, String> {
    let text = fs::read_to_string(config).map_err(|error| format!("{}: {error}", config.display()))?;
    let document = text.parse::<toml_edit::DocumentMut>().map_err(|error| format!("{}: {error}", config.display()))?;
    let mut rules = Vec::new();
    for table in document.get("set").and_then(|set| set.as_array_of_tables()).into_iter().flatten() {
        let field = |name: &str| table.get(name).and_then(|value| value.as_str());
        let path = field("path").ok_or("a [[set]] entry has no path")?.to_owned();
        let kind = match (field("kind"), field("key"), field("command")) {
            (Some("lines"), None, None) => match field("entry") {
                None | Some("line") => Kind::Lines(Identity::Line),
                Some("first-word") => Kind::Lines(Identity::FirstWord),
                Some(other) => return Err(format!("{path}: entry {other:?} must be line or first-word")),
            },
            (Some("block"), None, None) => match (field("start"), field("end")) {
                (Some(start), Some(end)) if !start.trim().is_empty() && !end.trim().is_empty() => Kind::Block { start: start.trim().to_owned(), end: end.trim().to_owned() },
                _ => return Err(format!("{path}: a block entry needs a start and an end line")),
            },
            (Some("toml-array"), Some(key), None) => {
                if let Some(Rule { kind: Kind::TomlArray(keys), .. }) = rules.iter_mut().find(|rule: &&mut Rule| rule.path == path && matches!(rule.kind, Kind::TomlArray(_))) {
                    keys.push(key.to_owned());
                    continue;
                }
                Kind::TomlArray(vec![key.to_owned()])
            }
            (Some("regenerate"), None, Some(command)) if !command.trim().is_empty() => Kind::Regenerate {
                command: command.to_owned(),
                sources: string_array(table, "sources").ok_or_else(|| format!("{path}: a regenerate entry needs a sources array of path prefixes"))?,
                generator: string_array(table, "generator").unwrap_or_default(),
            },
            (Some(kind), _, _) if !KINDS.contains(&kind) => {
                report_unknown(kind, config);
                continue;
            }
            _ => return Err(format!("{path}: kind must be lines, block with a start and an end, toml-array with a key, or regenerate with a command and sources")),
        };
        rules.push(Rule { path, kind });
    }
    Ok(rules)
}

fn merge_text(kind: &Kind, base: &str, ours: &str, theirs: &str) -> Result<String, String> {
    match kind {
        Kind::Lines(identity) => merge_lines(base, ours, theirs, *identity),
        Kind::Block { start, end } => block::merge_block(base, ours, theirs, start, end),
        Kind::TomlArray(keys) => merge_array_keys(base, ours, theirs, keys),
        Kind::Regenerate { command, .. } => Err(format!("a generated file: `north merge-driver continue` runs `{command}` on the merged sources")),
    }
}

fn conflict_text(base: &str, ours: &str, theirs: &str) -> String {
    match text_merge(base, ours, theirs) {
        Ok((text, true)) => text,
        _ => format!("<<<<<<< ours\n{ours}=======\n{theirs}>>>>>>> theirs\n"),
    }
}

pub fn drive(config: &Path, base: &Path, ours: &Path, theirs: &Path, path: &str) -> Result<(), String> {
    let read = |file: &Path| fs::read_to_string(file).map_err(|error| format!("{}: {error}", file.display()));
    let (base_text, ours_text, theirs_text) = (read(base)?, read(ours)?, read(theirs)?);
    let outcome = read_rules(config).and_then(|rules| match rules.iter().find(|rule| rule.path == path) {
        Some(rule) => merge_text(&rule.kind, &base_text, &ours_text, &theirs_text),
        None => match text_merge(&base_text, &ours_text, &theirs_text)? {
            (merged, false) => Ok(merged),
            (_, true) => Err(format!("{path} is not on the merge-set allowlist and git's text merge conflicts")),
        },
    });
    match outcome {
        Ok(merged) => fs::write(ours, merged).map_err(|error| error.to_string()),
        Err(reason) => {
            fs::write(ours, conflict_text(&base_text, &ours_text, &theirs_text)).map_err(|error| error.to_string())?;
            Err(reason)
        }
    }
}

pub(crate) fn repo_config() -> PathBuf {
    let top = Command::new("git").args(["rev-parse", "--show-toplevel"]).output().ok().filter(|output| output.status.success());
    top.map_or_else(|| PathBuf::from(DEFAULT_CONFIG), |output| Path::new(String::from_utf8_lossy(&output.stdout).trim()).join(DEFAULT_CONFIG))
}

fn config_flag(rest: &[String]) -> (Option<PathBuf>, &[String]) {
    match rest {
        [flag, path, tail @ ..] if flag == "--config" => (Some(PathBuf::from(path)), tail),
        _ => (None, rest),
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let Some((command, rest)) = arguments.split_first() else {
        eprintln!("{USAGE}");
        return 2;
    };
    let (config, rest) = config_flag(rest);
    let config = config.unwrap_or_else(repo_config);
    let (label, outcome) = match (command.as_str(), rest) {
        ("set", [base, ours, theirs, path]) => (path.as_str(), drive(&config, Path::new(base), Path::new(ours), Path::new(theirs), path)),
        ("continue", []) => ("continue", super::merge_continue::finish(Path::new("."), &config)),
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    match outcome {
        Ok(()) => 0,
        Err(reason) => {
            eprintln!("north merge-driver {command}: {label}: {reason}");
            1
        }
    }
}

#[path = "merge_set_block.rs"]
mod block;

#[cfg(test)]
#[path = "merge_set_tests.rs"]
mod tests;
