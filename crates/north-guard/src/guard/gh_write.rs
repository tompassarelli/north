use crate::shell::{ShellToken, basename, command, heredoc_bodies, operator_tokens, strip_heredocs};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const FILE_LIMIT: u64 = 1 << 20;
const PREFIXES: &[&str] = &["env", "exec", "nohup", "time"];
const ISSUE_VERBS: &[&str] = &["create", "comment", "edit", "review", "close", "reopen", "merge", "upload"];
const TEXT_FLAGS: &[&str] = &["--body", "-b", "--title", "-t", "--comment", "-c", "--notes", "-n", "--subject", "--description"];
const DESCRIPTION_FLAGS: &[&str] = &["-d", "--desc", "--description"];
const FILE_FLAGS: &[&str] = &["--body-file", "-F", "--notes-file"];
const FIELD_FLAGS: &[&str] = &["-f", "-F", "--field", "--raw-field"];
const API_VALUE_FLAGS: &[&str] = &[
    "-X", "--method", "-H", "--header", "-q", "--jq", "-t", "--template", "--hostname", "-p", "--preview", "--cache",
];
const GITHUB_URL: &str = "https://github.com/";

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
    pub opaque: bool,
}

pub fn writes(command_text: &str, cwd: Option<&str>) -> Vec<GhWrite> {
    let Ok(tokens) = operator_tokens(&strip_heredocs(command_text).replace('`', " ")) else {
        return Vec::new();
    };
    let mut walk = Walk {
        dir: cwd.map(str::to_owned),
        heredocs: heredoc_bodies(command_text)
            .into_iter()
            .map(|range| command_text[range].to_owned())
            .collect(),
        backticks: command_text.contains('`'),
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
}

impl Walk {
    fn finish(&mut self, operator: &str) {
        self.input_next = false;
        let segment = std::mem::take(&mut self.segment);
        let start = command(&segment, PREFIXES);
        let args = segment.get(start..).unwrap_or_default();
        let piped = std::mem::take(&mut self.piped);
        let inputs = std::mem::take(&mut self.inputs);
        if args.first().is_some_and(|word| word == "cd") {
            let target = args.iter().skip(1).find(|word| !word.starts_with('-'));
            self.dir = target.map(|word| resolve(word, self.dir.as_deref()).to_string_lossy().into_owned());
        } else if args.first().is_some_and(|word| basename(word) == "cat") && operator == "|" {
            self.piped = args.iter().skip(1).filter(|word| !word.starts_with('-')).cloned().collect();
        }
        let environment = segment[..start.min(segment.len())]
            .iter()
            .find_map(|word| word.strip_prefix("GH_REPO="))
            .map(str::to_owned);
        let Some(mut write) = parse(args, self.dir.as_deref()) else {
            return;
        };
        write.repo = write.repo.or(environment);
        write.dir = self.dir.clone();
        write.opaque |= self.backticks;
        for file in inputs.iter().chain(&piped) {
            push_file(&mut write, file, self.dir.as_deref());
        }
        write.bodies.extend(inputs);
        write.bodies.extend(self.heredocs.iter().cloned());
        self.out.push(write);
    }
}

fn parse(args: &[String], cwd: Option<&str>) -> Option<GhWrite> {
    if basename(args.first()?) != "gh" {
        return None;
    }
    let Scanned { mut write, group, positionals } = scan(&args[1..], cwd);
    let verb = positionals.first().copied();
    let group = group.filter(|group| writes_with(group, verb))?;
    complete(&mut write, group, &positionals, cwd);
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

fn scan<'a>(args: &'a [String], cwd: Option<&str>) -> Scanned<'a> {
    let mut scanned = Scanned { write: GhWrite::default(), group: None, positionals: Vec::new() };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let (flag, inline) = split_flag(arg);
        if matches!(flag, "-R" | "--repo") {
            scanned.write.repo = inline.or_else(|| rest.next().cloned());
        } else if !arg.starts_with('-') || arg == "-" {
            scanned.word(arg);
        } else if scanned.takes(flag) {
            let Some(value) = inline.or_else(|| rest.next().cloned()) else {
                break;
            };
            collect(&mut scanned.write, scanned.group, flag, &value, cwd);
        }
    }
    scanned
}

fn complete(write: &mut GhWrite, group: &str, positionals: &[&str], cwd: Option<&str>) {
    let verb = positionals.first().copied();
    if group == "gist" {
        for file in positionals.iter().skip(1) {
            push_file(write, file, cwd);
        }
    }
    if write.repo.is_none() {
        write.repo = positionals.iter().find_map(|word| target_repository(word));
    }
    if write.repo.is_none() && group == "repo" {
        write.repo = positionals.get(1).filter(|word| word.matches('/').count() == 1).map(|word| (*word).to_owned());
    }
    write.creates_issue = group == "issue" && verb == Some("create");
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
    if matches!(flag, "-c" | "--comment") {
        return verb == Some("close");
    }
    if group == Some("api") {
        return API_VALUE_FLAGS.contains(&flag) || FIELD_FLAGS.contains(&flag) || flag == "--input";
    }
    let described = matches!(group, Some("gist" | "repo" | "label")) && DESCRIPTION_FLAGS.contains(&flag);
    described || TEXT_FLAGS.contains(&flag) || FILE_FLAGS.contains(&flag)
}

fn collect(write: &mut GhWrite, group: Option<&str>, flag: &str, value: &str, cwd: Option<&str>) {
    if group == Some("api") {
        if flag == "--input" {
            push_file(write, value, cwd);
        } else if FIELD_FLAGS.contains(&flag)
            && let Some((_, field)) = value.split_once('=')
        {
            match field.strip_prefix('@') {
                Some(path) if flag != "-f" => push_file(write, path, cwd),
                _ => push_text(write, field, cwd),
            }
        }
    } else if FILE_FLAGS.contains(&flag) {
        push_file(write, value, cwd);
    } else if TEXT_FLAGS.contains(&flag) || DESCRIPTION_FLAGS.contains(&flag) {
        if matches!(flag, "-t" | "--title") {
            write.title = Some(value.to_owned());
        }
        push_text(write, value, cwd);
    }
}

fn push_text(write: &mut GhWrite, text: &str, cwd: Option<&str>) {
    for capture in CAT_SUBSTITUTION.captures_iter(text) {
        push_file(write, &capture[1], cwd);
    }
    let literal = CAT_SUBSTITUTION.replace_all(text, "");
    write.opaque |= EXPANSION.is_match(&literal);
    if !literal.trim().is_empty() {
        write.bodies.push(literal.into_owned());
    }
}

fn push_file(write: &mut GhWrite, path: &str, cwd: Option<&str>) {
    if path.is_empty() {
        return;
    }
    if path == "-" {
        write.opaque = true;
    } else if let Some(text) = read_bounded(&resolve(path, cwd)) {
        write.bodies.push(text);
    }
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
