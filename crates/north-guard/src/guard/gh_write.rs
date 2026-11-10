use crate::shell::{ShellToken, basename, command, heredoc_bodies, operator_tokens, strip_heredocs};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const FILE_LIMIT: u64 = 1 << 20;
const PREFIXES: &[&str] = &["env", "exec", "nohup", "time"];
const ISSUE_VERBS: &[&str] = &["create", "comment", "edit", "review", "close", "reopen", "merge", "upload"];
const TEXT_FLAGS: &[&str] = &["--body", "-b", "--title", "-t", "--comment", "-c", "--notes", "-n", "--subject", "--description"];
const DESCRIPTION_FLAGS: &[&str] = &["-d", "--desc", "--description"];
const FILE_FLAGS: &[&str] = &["--body-file", "-F", "--notes-file"];
const FIELD_FLAGS: &[&str] = &["-f", "-F", "--field", "--raw-field"];
const LABEL_FLAGS: &[&str] = &["-l", "--label"];
const API_VALUE_FLAGS: &[&str] = &[
    "-X", "--method", "-H", "--header", "-q", "--jq", "-t", "--template", "--hostname", "-p", "--preview", "--cache",
];
const GITHUB_URL: &str = "https://github.com/";
const READERS: &[&str] = &["cat", "sed", "head", "tail", "awk", "cut", "grep", "rg"];

static CAT_SUBSTITUTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\$\(\s*(?:cat\s+|<\s*)["']?([^)\s"']+)["']?\s*\)"#).unwrap());
static EXPANSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$[A-Za-z_{(]").unwrap());

#[derive(Debug, Default, PartialEq)]
pub struct GhWrite {
    pub repo: Option<String>,
    pub dir: Option<String>,
    pub public: bool,
    pub bodies: Vec<String>,
    pub title: Option<String>,
    pub creates_issue: bool,
    pub labels: Vec<String>,
    pub closes_issue: Option<String>,
    pub opaque: bool,
    pub edits: Option<(String, String)>,
    pub reason: Option<String>,
    pub files: Vec<PathBuf>,
    pub unread: bool,
    pub body_set: bool,
}

pub fn writes(command_text: &str, cwd: Option<&str>) -> Vec<GhWrite> {
    let Ok(tokens) = operator_tokens(&strip_heredocs(command_text).replace('`', " ")) else {
        return Vec::new();
    };
    let openers = tokens.iter().filter(|token| matches!(token, ShellToken::Operator(op) if op == "<<")).count();
    let mut walk = Walk {
        dir: cwd.map(str::to_owned),
        heredocs: heredoc_bodies(command_text)
            .into_iter()
            .map(|range| command_text[range].to_owned())
            .collect(),
        backticks: command_text.contains('`'),
        reliable: openers == heredoc_bodies(command_text).len(),
        ..Walk::default()
    };
    for token in tokens.into_iter().chain([ShellToken::Operator(";".into())]) {
        match token {
            ShellToken::Word(word) if walk.input_next => {
                walk.input_next = false;
                walk.inputs.push(word);
            }
            ShellToken::Word(word) => walk.segment.push(word),
            ShellToken::Operator(op) if op == "<" => walk.input_next = true,
            ShellToken::Operator(op) => walk.finish(&op),
        }
    }
    walk.out
}

#[derive(Default)]
struct Walk {
    dir: Option<String>,
    heredocs: Vec<String>,
    backticks: bool,
    out: Vec<GhWrite>,
    segment: Vec<String>,
    inputs: Vec<String>,
    piped: Vec<String>,
    input_next: bool,
    reliable: bool,
    openers: usize,
    last: String,
    pending: Option<usize>,
    written: Vec<(PathBuf, usize)>,
    built: Vec<PathBuf>,
    read: Vec<PathBuf>,
    printed: Vec<String>,
    vars: BTreeMap<String, String>,
}

impl Walk {
    fn finish(&mut self, operator: &str) {
        self.input_next = false;
        let preceded = std::mem::replace(&mut self.last, operator.to_owned());
        let heredoc = (operator == "<<").then(|| {
            self.openers += 1;
            self.openers - 1
        });
        let segment = std::mem::take(&mut self.segment);
        let segment = self.assign(segment);
        let start = command(&segment, PREFIXES);
        let args = segment.get(start..).unwrap_or_default();
        let piped = std::mem::take(&mut self.piped);
        let inputs = std::mem::take(&mut self.inputs);
        if args.first().is_some_and(|word| word == "cd") {
            let target = args.iter().skip(1).find(|word| !word.starts_with('-'));
            self.dir = target.map(|word| resolve(&expand(word, &self.vars), self.dir.as_deref()).to_string_lossy().into_owned());
        } else if args.first().is_some_and(|word| basename(word) == "cat") && operator == "|" {
            self.piped = args.iter().skip(1).filter(|word| !word.starts_with('-')).cloned().collect();
        }
        let environment = segment[..start.min(segment.len())]
            .iter()
            .find_map(|word| word.strip_prefix("GH_REPO="))
            .map(str::to_owned);
        let parsed = parse(args, self.dir.as_deref(), &self.vars);
        self.note_written(args, &preceded, operator, heredoc, parsed.is_some());
        if parsed.is_none() {
            self.note_source(args);
        }
        let Some(mut write) = parsed else {
            return;
        };
        write.repo = write.repo.or(environment);
        write.dir = self.dir.clone();
        write.opaque |= self.backticks;
        for file in inputs.iter().chain(&piped) {
            push_file(&mut write, file, self.dir.as_deref(), &self.vars);
        }
        write.bodies.extend(inputs);
        if self.reliable {
            write.bodies.extend(heredoc.map(|index| self.heredocs[index].clone()));
            for file in write.files.clone() {
                match self.written.iter().find(|(path, _)| *path == file) {
                    Some((_, index)) => write.bodies.push(self.heredocs[*index].clone()),
                    None if self.built.contains(&file) => {
                        write.unread = true;
                        write.bodies.extend(self.printed.iter().cloned());
                        write.bodies.extend(self.read.iter().filter(|path| **path != file && path.is_file()).filter_map(|path| read_bounded(path)));
                    }
                    None => write.unread |= read_bounded(&file).is_none(),
                }
            }
        } else {
            write.bodies.extend(self.heredocs.iter().cloned());
        }
        self.out.push(write);
    }

    /// Records a leading `NAME=value` word as a variable the command may use in a path, so a
    /// body file named by `$NAME` is read instead of judged from the command text alone
    /// (north#1041).
    fn assign(&mut self, segment: Vec<String>) -> Vec<String> {
        let mut words = Vec::new();
        let mut leading = true;
        for word in segment {
            if leading && let Some((name, value)) = assignment(&word) {
                let value = expand(value.trim_matches(['"', '\'']), &self.vars);
                self.vars.insert(name.to_owned(), value);
                continue;
            }
            leading = false;
            words.push(word);
        }
        words
    }

    fn note_source(&mut self, args: &[String]) {
        let args = args.strip_prefix(&["{".to_owned()]).or_else(|| args.strip_prefix(&["(".to_owned()])).unwrap_or(args);
        let Some((name, operands)) = args.split_first() else { return };
        let operands = operands.iter().filter(|word| !word.starts_with('-'));
        match basename(name) {
            "printf" | "echo" => self.printed.extend(operands.map(|word| word.replace("\\n", "\n"))),
            name if READERS.contains(&name) => {
                let dir = self.dir.clone();
                self.read.extend(operands.map(|word| expand(word, &self.vars)).filter(|word| !word.contains('$')).map(|word| resolve(&word, dir.as_deref())));
            }
            _ => {}
        }
    }

    fn note_written(&mut self, args: &[String], preceded: &str, operator: &str, heredoc: Option<usize>, is_gh: bool) {
        let redirected = matches!(preceded, ">" | ">>");
        let owner = heredoc.filter(|_| !is_gh).or(self.pending.filter(|_| redirected));
        if let Some(index) = owner {
            let file = if redirected {
                args.first()
            } else {
                args.first().filter(|word| basename(word) == "tee").and_then(|_| args.iter().skip(1).rfind(|word| !word.starts_with('-')))
            };
            match file {
                Some(file) => {
                    let path = resolve(&expand(file, &self.vars), self.dir.as_deref());
                    self.written.push((path, index));
                    self.pending = None;
                }
                None => self.pending = Some(index),
            }
        } else if redirected && let Some(file) = args.first() {
            self.built.push(resolve(&expand(file, &self.vars), self.dir.as_deref()));
        }
        if !matches!(operator, ">" | ">>" | "<<") {
            self.pending = None;
        }
    }
}

fn parse(args: &[String], cwd: Option<&str>, vars: &BTreeMap<String, String>) -> Option<GhWrite> {
    if basename(args.first()?) != "gh" {
        return None;
    }
    let Scanned { mut write, group, positionals } = scan(&args[1..], cwd, vars);
    let verb = positionals.first().copied();
    let group = group.filter(|group| writes_with(group, verb))?;
    complete(&mut write, group, &positionals, cwd, vars);
    Some(write)
}

struct Scanned<'a> {
    write: GhWrite,
    group: Option<&'a str>,
    positionals: Vec<&'a str>,
}

impl<'a> Scanned<'a> {
    fn word(&mut self, arg: &'a str) {
        match self.group {
            None => self.group = Some(arg),
            Some(_) => self.positionals.push(arg),
        }
    }

    fn takes(&self, flag: &str) -> bool {
        takes_value(self.group, self.positionals.first().copied(), flag)
    }
}

fn split_flag(arg: &str) -> (&str, Option<String>) {
    match arg.split_once('=') {
        Some((flag, value)) if arg.starts_with("--") => (flag, Some(value.to_owned())),
        _ => (arg, None),
    }
}

fn scan<'a>(args: &'a [String], cwd: Option<&str>, vars: &BTreeMap<String, String>) -> Scanned<'a> {
    let mut scanned = Scanned { write: GhWrite::default(), group: None, positionals: Vec::new() };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let (flag, inline) = split_flag(arg);
        if matches!(flag, "-R" | "--repo") {
            scanned.write.repo = inline.or_else(|| rest.next().cloned());
        } else if !arg.starts_with('-') || arg == "-" {
            scanned.word(arg);
        } else if scanned.takes(flag) {
            scanned.write.body_set |= scanned.group == Some("issue") && matches!(flag, "--body" | "-b" | "--body-file" | "-F");
            let Some(value) = inline.or_else(|| rest.next().cloned()) else {
                break;
            };
            collect(&mut scanned.write, scanned.group, flag, &value, cwd, vars);
        }
    }
    scanned
}

fn complete(write: &mut GhWrite, group: &str, positionals: &[&str], cwd: Option<&str>, vars: &BTreeMap<String, String>) {
    let verb = positionals.first().copied();
    if group == "gist" {
        for file in positionals.iter().skip(1) {
            push_file(write, file, cwd, vars);
        }
    }
    if write.repo.is_none() {
        write.repo = positionals.iter().find_map(|word| target_repository(word));
    }
    if write.repo.is_none() && group == "repo" {
        write.repo = positionals.get(1).filter(|word| word.matches('/').count() == 1).map(|word| (*word).to_owned());
    }
    write.creates_issue = group == "issue" && verb == Some("create");
    write.closes_issue = (group == "issue" && verb == Some("close")).then(|| positionals.get(1).map(|word| (*word).to_owned()).unwrap_or_default());
    write.edits = positionals.get(1).filter(|_| matches!(group, "issue" | "pr") && verb == Some("edit")).map(|word| (group.to_owned(), (*word).to_owned()));
    write.public = match group {
        "gist" => true,
        "repo" => verb == Some("create"),
        "api" => write.repo.is_none() && !verb.is_some_and(|path| path.contains('{')),
        _ => false,
    };
}

fn writes_with(group: &str, verb: Option<&str>) -> bool {
    match group {
        "api" => true,
        "issue" | "pr" | "release" => verb.is_some_and(|verb| ISSUE_VERBS.contains(&verb)),
        "gist" | "repo" | "label" => verb.is_some_and(|verb| matches!(verb, "create" | "edit")),
        _ => false,
    }
}

fn takes_value(group: Option<&str>, verb: Option<&str>, flag: &str) -> bool {
    if matches!(flag, "-r" | "--reason") {
        return group == Some("issue") && verb == Some("close");
    }
    if matches!(flag, "-c" | "--comment") {
        return verb == Some("close");
    }
    if group == Some("api") {
        return API_VALUE_FLAGS.contains(&flag) || FIELD_FLAGS.contains(&flag) || flag == "--input";
    }
    let labelled = group == Some("issue") && LABEL_FLAGS.contains(&flag);
    let described = matches!(group, Some("gist" | "repo" | "label")) && DESCRIPTION_FLAGS.contains(&flag);
    described || labelled || TEXT_FLAGS.contains(&flag) || FILE_FLAGS.contains(&flag)
}

fn collect(write: &mut GhWrite, group: Option<&str>, flag: &str, value: &str, cwd: Option<&str>, vars: &BTreeMap<String, String>) {
    if group == Some("api") {
        if flag == "--input" {
            push_file(write, value, cwd, vars);
        } else if FIELD_FLAGS.contains(&flag)
            && let Some((_, field)) = value.split_once('=')
        {
            match field.strip_prefix('@') {
                Some(path) if flag != "-f" => push_file(write, path, cwd, vars),
                _ => push_text(write, field, cwd, vars),
            }
        }
    } else if matches!(flag, "-r" | "--reason") {
        write.reason = Some(value.to_owned());
    } else if group == Some("issue") && LABEL_FLAGS.contains(&flag) {
        write.labels.extend(value.split(',').map(str::trim).filter(|label| !label.is_empty()).map(str::to_owned));
    } else if FILE_FLAGS.contains(&flag) {
        push_file(write, value, cwd, vars);
    } else if TEXT_FLAGS.contains(&flag) || DESCRIPTION_FLAGS.contains(&flag) {
        if matches!(flag, "-t" | "--title") {
            write.title = Some(value.to_owned());
        }
        push_text(write, value, cwd, vars);
    }
}

fn push_text(write: &mut GhWrite, text: &str, cwd: Option<&str>, vars: &BTreeMap<String, String>) {
    for capture in CAT_SUBSTITUTION.captures_iter(text) {
        push_file(write, &capture[1], cwd, vars);
    }
    let literal = CAT_SUBSTITUTION.replace_all(text, "");
    write.opaque |= EXPANSION.is_match(&literal);
    if !literal.trim().is_empty() {
        write.bodies.push(literal.into_owned());
    }
}

fn push_file(write: &mut GhWrite, path: &str, cwd: Option<&str>, vars: &BTreeMap<String, String>) {
    if path.is_empty() {
        return;
    }
    if path == "-" {
        write.opaque = true;
    } else {
        let resolved = resolve(&expand(path, vars), cwd);
        write.files.push(resolved.clone());
        if let Some(text) = read_bounded(&resolved) {
            write.bodies.push(text);
        }
    }
}

/// A leading shell assignment, `NAME=value`, whose value a later word may name as `$NAME`.
fn assignment(word: &str) -> Option<(&str, &str)> {
    let (name, value) = word.split_once('=')?;
    let mut chars = name.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic() || first == '_').then_some(())?;
    chars.all(|character| character.is_ascii_alphanumeric() || character == '_').then_some((name, value))
}

/// `word` with `$NAME` and `${NAME}` replaced from the command's own assignments, then the
/// environment, so a body file a command names through a variable is read (north#1041).
fn expand(word: &str, vars: &BTreeMap<String, String>) -> String {
    let chars: Vec<char> = word.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    while at < chars.len() {
        match chars[at] {
            '$' => match variable(&chars, at + 1, vars) {
                Some((value, next)) => {
                    out.push_str(&value);
                    at = next;
                }
                None => {
                    out.push('$');
                    at += 1;
                }
            },
            character => {
                out.push(character);
                at += 1;
            }
        }
    }
    out
}

/// The value `$NAME` or `${NAME}` names at `start` and the index after it, or none when the word holds no
/// such variable or nothing defines it; an unset name keeps its `$` so the command text stays as written.
fn variable(chars: &[char], start: usize, vars: &BTreeMap<String, String>) -> Option<(String, usize)> {
    if start >= chars.len() {
        return None;
    }
    let (name, next) = if chars[start] == '{' {
        chars[start + 1..].iter().position(|character| *character == '}').map_or((String::new(), start), |end| (chars[start + 1..start + 1 + end].iter().collect(), start + 2 + end))
    } else {
        let mut end = start;
        while end < chars.len() && (chars[end] == '_' || chars[end].is_ascii_alphanumeric()) {
            end += 1;
        }
        (chars[start..end].iter().collect(), end)
    };
    if name.is_empty() {
        return None;
    }
    let value = vars.get(&name).cloned().or_else(|| std::env::var(&name).ok())?;
    Some((value, next))
}

fn resolve(path: &str, cwd: Option<&str>) -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let path = match path.strip_prefix("~/") {
        Some(tail) => home().join(tail),
        None => PathBuf::from(path),
    };
    match cwd {
        Some(cwd) if path.is_relative() => Path::new(cwd).join(path),
        _ => path,
    }
}

fn read_bounded(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut buffer = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(FILE_LIMIT)
        .read_to_end(&mut buffer)
        .ok()?;
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

fn target_repository(word: &str) -> Option<String> {
    if let Some(url) = word.strip_prefix(GITHUB_URL) {
        let mut parts = url.split('/');
        return Some(format!("{}/{}", parts.next()?, parts.next()?));
    }
    let mut parts = word.trim_start_matches('/').split('/');
    (parts.next()? == "repos").then_some(())?;
    let (owner, name) = (parts.next()?, parts.next()?);
    (!owner.contains('{') && !name.contains('{')).then(|| format!("{owner}/{name}"))
}

#[cfg(test)]
#[path = "gh_write_tests.rs"]
mod tests;
