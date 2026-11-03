use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::capacity::build_offload::Declined;
use crate::shell;

static MOVED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?s)^\s*cd\s+([^\s;&|<>()$`'"\\]+)\s*&&\s*(.*)$"#).unwrap());
static REDIRECTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?s)^(.*\S)\s+(&>>?|1?>>?)\s*([^\s;&|<>()$`'"\\]+)(\s+2>&1)?\s*$"#).unwrap());
static ELSEWHERE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#">>?\|?\s*(["']?[~/$][^\s;&|<>()"']*)"#).unwrap());

#[derive(Debug, PartialEq)]
pub(super) struct Sink {
    pub(super) path: PathBuf,
    pub(super) append: bool,
}

impl Sink {
    pub(super) fn open(&self) -> std::io::Result<File> {
        OpenOptions::new().create(true).write(true).append(self.append).truncate(!self.append).open(&self.path)
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct Plan {
    pub(super) directory: PathBuf,
    pub(super) command: Vec<String>,
    pub(super) sink: Option<Sink>,
}

fn resolve(word: &str, base: &Path, home: &Path) -> PathBuf {
    match word.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(rest) if rest.starts_with('/') => home.join(rest.trim_start_matches('/')),
        _ => base.join(word),
    }
}

fn single(body: &str) -> bool {
    let grouped = body.starts_with('{') && body.ends_with('}') && body.matches('{').count() == 1 && body.matches('}').count() == 1;
    grouped || shell::shell_segments_sep(body).len() == 1
}

fn redirected(body: &str, directory: &Path, home: &Path) -> Option<(String, Sink)> {
    let found = REDIRECTED.captures(body)?;
    let (rest, operator, target) = (&found[1], &found[2], &found[3]);
    if !single(rest) || rest.ends_with("2>&1") || target.starts_with("/dev/") {
        return None;
    }
    let merged = operator.starts_with('&') || found.get(4).is_some();
    let command = if merged { format!("{rest} 2>&1") } else { rest.to_owned() };
    Some((command, Sink { path: resolve(target, directory, home), append: operator.ends_with(">>") }))
}

fn writes_elsewhere(body: &str) -> Option<String> {
    ELSEWHERE.captures_iter(body).map(|found| found[1].trim_matches(['"', '\'']).to_owned()).find(|target| !matches!(target.as_str(), "/dev/null" | "/dev/stdout" | "/dev/stderr"))
}

pub(super) fn plan(command: &[String], cwd: &Path, home: &Path) -> Result<Plan, Declined> {
    let unchanged = || Plan { directory: cwd.to_path_buf(), command: command.to_vec(), sink: None };
    let [interpreter, flag, script] = command else { return Ok(unchanged()) };
    let shell_flag = flag.strip_prefix('-').is_some_and(|letters| letters.contains('c') && letters.chars().all(|letter| letter.is_ascii_alphabetic()));
    if !matches!(shell::basename(interpreter), "bash" | "sh") || !shell_flag {
        return Ok(unchanged());
    }
    let (directory, body) = MOVED.captures(script).map_or_else(|| (cwd.to_path_buf(), script.clone()), |found| (resolve(&found[1], cwd, home), found[2].to_owned()));
    let (body, sink) = redirected(&body, &directory, home).map_or((body, None), |(body, sink)| (body, Some(sink)));
    if let Some(target) = writes_elsewhere(&body) {
        return Err(Declined::refused("laptop redirect", format!("the command writes {target}, which would land on the host; end it with one top-level redirect so the laptop writes it")));
    }
    Ok(Plan { directory, command: vec![interpreter.clone(), flag.clone(), body], sink })
}
