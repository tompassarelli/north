use crate::shell::{ShellToken, operator_tokens};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use crate::platform::git as platform_git;

const ALLOWLIST: &str = ".language-allowlist.json";
const ALTERNATIVE: &str = "write it in Rust under src/ or as a north subcommand";
const INPUT_LIMIT: u64 = 1024 * 1024;
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Class {
    Python,
    Script,
    Shell,
}

fn classify(path: &Path, content: &[u8]) -> Option<Class> {
    match path.extension().and_then(|value| value.to_str()) {
        Some("py") => Some(Class::Python),
        Some("ts" | "tsx" | "mts" | "cts" | "js" | "mjs" | "cjs") => Some(Class::Script),
        Some("sh" | "bash") => Some(Class::Shell),
        None => {
            let line = content
                .split(|byte| *byte == b'\n')
                .next()
                .unwrap_or_default();
            let line = std::str::from_utf8(line).ok()?.strip_prefix("#!")?;
            let mut words = line.split_whitespace();
            let executable = Path::new(words.next()?).file_name()?.to_str()?;
            let executable = if executable == "env" {
                words.find(|word| !word.starts_with('-') && !word.contains('='))?
            } else {
                executable
            };
            match executable {
                "python" | "python3" => Some(Class::Python),
                "sh" | "bash" | "zsh" | "dash" => Some(Class::Shell),
                _ => None,
            }
        }
        _ => None,
    }
}

#[derive(Clone, Debug, Deserialize)]
struct Entry {
    glob: String,
    class: Class,
    gap: String,
    provenance: String,
    port_owner: Option<String>,
    port_target: Option<String>,
    commitment: Option<String>,
    created: Option<String>,
}

#[derive(Default, Deserialize)]
struct Allowlist {
    #[serde(default)]
    ratchet: Vec<Class>,
    entries: Vec<Entry>,
}

impl Allowlist {
    fn parse(bytes: &[u8]) -> Result<Self> {
        let list: Self =
            serde_json::from_slice(bytes).map_err(|error| format!("{ALLOWLIST}: {error}"))?;
        for entry in &list.entries {
            if entry.gap.trim().is_empty()
                || entry.provenance.trim().is_empty()
                || entry.glob.is_empty()
            {
                return Err(format!(
                    "{ALLOWLIST}: invalid entry: glob, gap and provenance must be nonempty"
                ));
            }
        }
        Ok(list)
    }

    fn allows(&self, path: &str, class: Class) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.class == class && glob_matches(&entry.glob, path))
    }
}

#[derive(Clone, Copy)]
enum Token {
    Literal(char),
    Any,
    Star,
    All,
    Directory,
    DirectoryBody,
}

fn tokens(glob: &str) -> Vec<Token> {
    let mut chars = glob.chars().peekable();
    let mut result = Vec::new();
    while let Some(ch) = chars.next() {
        let token = match ch {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                    result.push(Token::Directory);
                    Token::DirectoryBody
                } else {
                    Token::All
                }
            }
            '*' => Token::Star,
            '?' => Token::Any,
            _ => Token::Literal(ch),
        };
        result.push(token);
    }
    result
}

fn closure(pattern: &[Token], mut states: BTreeSet<usize>) -> BTreeSet<usize> {
    let mut pending: Vec<_> = states.iter().copied().collect();
    while let Some(index) = pending.pop() {
        let next = match pattern.get(index) {
            Some(Token::Star | Token::All) => Some(index + 1),
            Some(Token::Directory) => Some(index + 2),
            _ => None,
        };
        if let Some(next) = next
            && states.insert(next)
        {
            pending.push(next);
        }
    }
    states
}

fn advance(pattern: &[Token], states: &BTreeSet<usize>, ch: char) -> BTreeSet<usize> {
    let mut next = BTreeSet::new();
    for &index in states {
        match pattern.get(index) {
            Some(Token::Literal(expected)) if ch == *expected => {
                next.insert(index + 1);
            }
            Some(Token::Any) if ch != '/' => {
                next.insert(index + 1);
            }
            Some(Token::Star) if ch != '/' => {
                next.insert(index);
            }
            Some(Token::All) => {
                next.insert(index);
            }
            Some(Token::Directory) => {
                next.insert(index + 1);
                if ch == '/' {
                    next.insert(index);
                }
            }
            Some(Token::DirectoryBody) => {
                next.insert(index);
                if ch == '/' {
                    next.insert(index - 1);
                }
            }
            _ => {}
        }
    }
    closure(pattern, next)
}

fn glob_matches(glob: &str, path: &str) -> bool {
    let pattern = tokens(glob);
    let states = path
        .chars()
        .fold(closure(&pattern, BTreeSet::from([0])), |states, ch| {
            advance(&pattern, &states, ch)
        });
    states.contains(&pattern.len())
}

fn covered_by(entry: &Entry, old: &Allowlist) -> bool {
    let new = tokens(&entry.glob);
    let old: Vec<_> = old
        .entries
        .iter()
        .filter(|other| other.class == entry.class)
        .map(|other| tokens(&other.glob))
        .collect();
    let mut alphabet = BTreeSet::from(['/']);
    for token in new.iter().chain(old.iter().flatten()) {
        if let Token::Literal(ch) = token {
            alphabet.insert(*ch);
        }
    }
    if let Some(other) = (1..=0x10ffff)
        .filter_map(char::from_u32)
        .find(|ch| !alphabet.contains(ch))
    {
        alphabet.insert(other);
    }
    let start = (
        closure(&new, BTreeSet::from([0])),
        old.iter()
            .map(|pattern| closure(pattern, BTreeSet::from([0])))
            .collect::<Vec<_>>(),
    );
    let mut seen = BTreeSet::from([start.clone()]);
    let mut queue = VecDeque::from([start]);
    while let Some((new_states, old_states)) = queue.pop_front() {
        if new_states.contains(&new.len())
            && !old
                .iter()
                .zip(&old_states)
                .any(|(pattern, states)| states.contains(&pattern.len()))
        {
            return false;
        }
        for &ch in &alphabet {
            let next = (
                advance(&new, &new_states, ch),
                old.iter()
                    .zip(&old_states)
                    .map(|(pattern, states)| advance(pattern, states, ch))
                    .collect::<Vec<_>>(),
            );
            if !next.0.is_empty() && seen.insert(next.clone()) {
                queue.push_back(next);
            }
        }
    }
    true
}

fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let literal: Vec<&str> = ["--literal-pathspecs"].into_iter().chain(args.iter().copied()).collect();
    platform_git::stdout(repo, &literal, |output| {
        format!(
            "git {} failed: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr).trim()
        )
    })
}

fn blob(repo: &Path, rev: &str, path: &str) -> Result<Option<Vec<u8>>> {
    let tree = git(repo, &["ls-tree", "-z", rev, "--", path])?;
    if tree.is_empty() {
        return Ok(None);
    }
    let object = format!("{rev}:{path}");
    let kind = git(repo, &["cat-file", "-t", &object])?;
    if kind != b"blob\n" {
        return Ok(None);
    }
    git(repo, &["show", &object]).map(Some)
}

fn revision_allowlist(repo: &Path, rev: &str) -> Result<Allowlist> {
    blob(repo, rev, ALLOWLIST)?
        .map(|bytes| Allowlist::parse(&bytes))
        .unwrap_or_else(|| Ok(Allowlist::default()))
}

fn line_count(bytes: &[u8]) -> usize {
    bytes.iter().filter(|byte| **byte == b'\n').count()
        + usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n"))
}

fn shell_code_lines(bytes: &[u8]) -> usize {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| {
            let line = line.trim_ascii_start();
            !line.is_empty() && !line.starts_with(b"#")
        })
        .count()
}

fn has_approval(repo: &Path, base: &str, head: &str) -> Result<bool> {
    let log = git(
        repo,
        &[
            "log",
            "--format=%(trailers:key=Language-allowlist,valueonly)",
            &format!("{base}..{head}"),
        ],
    )?;
    Ok(String::from_utf8_lossy(&log).lines().any(|line| {
        let mut words = line.split_whitespace();
        words.next() == Some("Tom")
            && words.next().is_some_and(|date| !date.is_empty())
            && words.next().is_some()
    }))
}

fn calendar_date(value: &str) -> Option<time::Date> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return None;
    }
    time::Date::from_calendar_date(
        value[..4].parse().ok()?,
        time::Month::try_from(value[5..7].parse::<u8>().ok()?).ok()?,
        value[8..].parse().ok()?,
    )
    .ok()
}

fn python_entry_problem(
    repo: &Path,
    head: &str,
    date: time::Date,
    entry: &Entry,
) -> Result<Option<&'static str>> {
    for (field, value) in [
        ("port_owner", &entry.port_owner),
        ("port_target", &entry.port_target),
        ("commitment", &entry.commitment),
        ("created", &entry.created),
    ] {
        if value.as_ref().is_none_or(|value| value.trim().is_empty()) {
            return Ok(Some(field));
        }
    }
    let target = entry.port_target.as_deref().unwrap();
    if !target.ends_with(".rs")
        || target.contains('\\')
        || target
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !Path::new(target)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Ok(Some("port_target (must be a repo-relative Rust path)"));
    }
    if blob(repo, head, target)?.is_some() {
        return Ok(Some("port_target (expired: Rust port already landed)"));
    }
    let Some(created) = entry.created.as_deref().and_then(calendar_date) else {
        return Ok(Some("created (must be YYYY-MM-DD)"));
    };
    if (date - created).whole_days() > 7 {
        return Ok(Some(
            "created (expired: more than 7 days before head commit)",
        ));
    }
    Ok(None)
}

fn violations(repo: &Path, base: &str, head: &str) -> Result<Vec<String>> {
    let base = if base == EMPTY_TREE {
        EMPTY_TREE.to_string()
    } else {
        String::from_utf8(git(
            repo,
            &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
        )?)
        .map_err(|error| error.to_string())?
    };
    let head = String::from_utf8(git(
        repo,
        &["rev-parse", "--verify", &format!("{head}^{{commit}}")],
    )?)
    .map_err(|error| error.to_string())?;
    let (base, head) = (base.trim(), head.trim());
    let old_list = revision_allowlist(repo, base)?;
    let list = revision_allowlist(repo, head)?;
    let mut failures = Vec::new();
    if list
        .entries
        .iter()
        .any(|entry| entry.class == Class::Python)
    {
        let date = git(repo, &["show", "-s", "--format=%cs", head])?;
        let date = calendar_date(String::from_utf8_lossy(&date).trim())
            .ok_or("invalid head committer date")?;
        for entry in list
            .entries
            .iter()
            .filter(|entry| entry.class == Class::Python)
        {
            if let Some(field) = python_entry_problem(repo, head, date, entry)? {
                failures.push(format!("{ALLOWLIST}: R4 Python entry {}: missing, invalid or expired {field}; remove the entry or supply a valid Rust port commitment; {ALTERNATIVE}", entry.glob));
            }
        }
    }
    if (old_list.ratchet.contains(&Class::Script) && !list.ratchet.contains(&Class::Script)
        || list
        .entries
        .iter()
        .any(|entry| entry.class != Class::Python && !covered_by(entry, &old_list)))
        && !has_approval(repo, base, head)?
    {
        failures.push(format!("{ALLOWLIST}: R3 allowlist gained or widened an entry or removed the script ratchet without Language-allowlist: Tom <date> <quote>; {ALTERNATIVE}"));
    }
    let diff = git(
        repo,
        &[
            "diff",
            "--name-status",
            "-z",
            "-M",
            "-C",
            "--find-copies-harder",
            base,
            head,
            "--",
        ],
    )?;
    let mut fields = diff
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    while let Some(status) = fields.next() {
        let status = status[0];
        let first = fields.next().ok_or("missing git diff path")?;
        let (old_path, new_path) = if matches!(status, b'R' | b'C') {
            (first, fields.next().ok_or("missing git diff destination")?)
        } else {
            (first, first)
        };
        if status == b'D' {
            continue;
        }
        let old_path = std::str::from_utf8(old_path).map_err(|error| error.to_string())?;
        let path = std::str::from_utf8(new_path).map_err(|error| error.to_string())?;
        let Some(new) = blob(repo, head, path)? else {
            continue;
        };
        let Some(class) = classify(Path::new(path), &new) else {
            continue;
        };
        let old = if matches!(status, b'A' | b'C') {
            None
        } else {
            blob(repo, base, old_path)?
        };
        let old_class = old
            .as_ref()
            .and_then(|content| classify(Path::new(old_path), content));
        let new_python = class == Class::Python
            && (matches!(status, b'A' | b'C')
                || (status == b'R' && old_class != Some(Class::Python)));
        if (class == Class::Script && !list.ratchet.contains(&Class::Script))
            || (!new_python && list.allows(path, class))
        {
            continue;
        }
        let rule = if class == Class::Shell {
            let before = old.as_ref().filter(|_| old_class == Some(Class::Shell))
                .map_or(0, |bytes| shell_code_lines(bytes));
            let after = shell_code_lines(&new);
            (after > 80 && after > before)
                .then_some("R2 Shell code-line count grew past 80 without an allowlist entry")
        } else if new_python
            || status == b'A'
            || (matches!(status, b'R' | b'C') && old_class != Some(class))
        {
            Some("R1 new Python path is forbidden or Script path is not allowlisted")
        } else if line_count(&new) > old.as_ref().map_or(0, |bytes| line_count(bytes)) {
            Some("R2 Python or Script line count increased without an allowlist entry")
        } else {
            None
        };
        if let Some(rule) = rule {
            failures.push(format!("{path}: {rule}; {ALTERNATIVE}"));
        }
    }
    Ok(failures)
}

pub fn run_gate(args: &[String]) -> ExitCode {
    let result = (|| {
        let mut repo = None;
        let mut base = None;
        let mut head = None;
        let mut args = args.iter();
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .ok_or("Usage: north language-gate --repo DIR --base REV --head REV")?;
            let slot = match flag.as_str() {
                "--repo" => &mut repo,
                "--base" => &mut base,
                "--head" => &mut head,
                _ => return Err(format!("unknown language-gate option {flag}")),
            };
            if slot.replace(value.as_str()).is_some() {
                return Err(format!("duplicate option {flag}"));
            }
        }
        let (Some(repo), Some(base), Some(head)) = (repo, base, head) else {
            return Err("Usage: north language-gate --repo DIR --base REV --head REV".into());
        };
        violations(Path::new(repo), base, head)
    })();
    match result {
        Ok(failures) if failures.is_empty() => {
            println!("language-gate: passed");
            ExitCode::SUCCESS
        }
        Ok(failures) => {
            for failure in failures {
                println!("{failure}");
            }
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("language-gate: {error}");
            ExitCode::FAILURE
        }
    }
}

fn resolve(path: &str, cwd: &Path, home: &Path) -> Result<PathBuf> {
    let path = if let Some(suffix) = path
        .strip_prefix("~/")
        .or_else(|| path.strip_prefix("$HOME/"))
        .or_else(|| path.strip_prefix("${HOME}/"))
    {
        home.join(suffix)
    } else {
        cwd.join(path)
    };
    let mut normalized = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            _ => normalized.push(part),
        }
    }
    let mut ancestor = normalized.as_path();
    let mut missing = Vec::new();
    while !ancestor.try_exists().map_err(|error| error.to_string())? {
        missing.push(ancestor.file_name().ok_or("unresolved path")?.to_owned());
        ancestor = ancestor.parent().ok_or("unresolved parent")?;
    }
    let mut canonical = ancestor.canonicalize().map_err(|error| error.to_string())?;
    for part in missing.into_iter().rev() {
        canonical.push(part);
    }
    Ok(canonical)
}

fn shell_targets(command: &str, cwd: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let tokens = operator_tokens(command)?;
    let mut targets = Vec::new();
    for pair in tokens.windows(2) {
        if let [ShellToken::Operator(operator), ShellToken::Word(path)] = pair
            && matches!(operator.as_str(), ">" | ">>")
        {
            targets.push((path.clone(), Vec::new()));
        }
    }
    let mut words = Vec::new();
    let mut redirect_operand = false;
    for token in tokens
        .iter()
        .chain(std::iter::once(&ShellToken::Operator(";".into())))
    {
        match token {
            ShellToken::Word(_) if redirect_operand => redirect_operand = false,
            ShellToken::Word(word) => words.push(word.as_str()),
            ShellToken::Operator(operator)
                if matches!(
                    operator.as_str(),
                    ";" | "&&" | "||" | "|" | "\n" | "(" | ")" | "&"
                ) =>
            {
                let start = words
                    .iter()
                    .position(|word| !word.contains('='))
                    .unwrap_or(words.len());
                if let Some(name) = words
                    .get(start)
                    .and_then(|word| Path::new(word).file_name())
                    .and_then(|word| word.to_str())
                {
                    let operands: Vec<_> = words[start + 1..]
                        .iter()
                        .filter(|word| !word.starts_with('-'))
                        .copied()
                        .collect();
                    match name {
                        "touch" | "tee" => {
                            for path in operands {
                                targets.push((path.into(), Vec::new()));
                            }
                        }
                        "cp" | "mv" | "install" | "ln" => {
                            let directory = words
                                .windows(2)
                                .find(|pair| matches!(pair[0], "-t" | "--target-directory"))
                                .map(|pair| pair[1])
                                .or_else(|| {
                                    words
                                        .iter()
                                        .find_map(|word| word.strip_prefix("--target-directory="))
                                });
                            if let Some(target) = directory {
                                for source in operands.iter().filter(|source| **source != target) {
                                    if let Some(name) = Path::new(source).file_name() {
                                        targets.push((
                                            Path::new(target)
                                                .join(name)
                                                .to_string_lossy()
                                                .into_owned(),
                                            fs::read(cwd.join(source)).unwrap_or_default(),
                                        ));
                                    }
                                }
                            } else if let Some(target) = operands.last() {
                                if cwd.join(target).is_dir() {
                                    for source in operands.iter().take(operands.len() - 1) {
                                        if let Some(name) = Path::new(source).file_name() {
                                            targets.push((
                                                Path::new(target)
                                                    .join(name)
                                                    .to_string_lossy()
                                                    .into_owned(),
                                                fs::read(cwd.join(source)).unwrap_or_default(),
                                            ));
                                        }
                                    }
                                } else {
                                    let content = operands
                                        .first()
                                        .and_then(|source| fs::read(cwd.join(source)).ok())
                                        .unwrap_or_default();
                                    targets.push(((*target).into(), content));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                words.clear();
                redirect_operand = false;
            }
            ShellToken::Operator(operator)
                if matches!(operator.as_str(), ">" | ">>" | "<" | "<<") =>
            {
                redirect_operand = true
            }
            _ => {}
        }
    }
    Ok(targets)
}

fn patch_envelope(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) if text.contains("*** Begin Patch") => Some(text),
        Value::Object(fields) => fields.values().find_map(patch_envelope),
        _ => None,
    }
}

fn guard_decision(input: &[u8], home: &Path, cwd: &Path, disabled: bool) -> Result<Option<Value>> {
    if disabled || input.len() > INPUT_LIMIT as usize {
        return Ok(None);
    }
    let payload: Value = serde_json::from_slice(input).map_err(|error| error.to_string())?;
    let tool = payload
        .get("tool_name")
        .and_then(Value::as_str)
        .ok_or("missing tool name")?;
    let raw = payload.get("tool_input").ok_or("missing tool input")?;
    let decoded;
    let fields = if let Some(raw) = raw.as_str() {
        decoded = serde_json::from_str::<Value>(raw).ok();
        decoded.as_ref().unwrap_or(&Value::Null)
    } else {
        raw
    };
    let cwd = fields
        .get("workdir")
        .or_else(|| fields.get("cwd"))
        .or_else(|| payload.get("cwd"))
        .and_then(Value::as_str)
        .map(Path::new)
        .unwrap_or(cwd);
    let mut targets = Vec::new();
    match tool {
        "Write" | "NotebookEdit" | "Edit" | "MultiEdit" => {
            if let Some(path) = fields
                .get("file_path")
                .or_else(|| fields.get("filePath"))
                .or_else(|| fields.get("notebook_path"))
                .and_then(Value::as_str)
            {
                targets.push((
                    path.into(),
                    fields
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .as_bytes()
                        .to_vec(),
                ));
            }
        }
        name if name.ends_with("apply_patch") => {
            let patch = patch_envelope(fields)
                .or_else(|| patch_envelope(raw))
                .ok_or("missing patch")?;
            let mut current: Option<(String, Vec<u8>)> = None;
            for line in patch.lines() {
                if line.starts_with("*** ") {
                    if let Some(target) = current.take() {
                        targets.push(target);
                    }
                    if let Some(path) = line.strip_prefix("*** Add File: ") {
                        current = Some((path.into(), Vec::new()));
                    }
                } else if let Some(content) = line.strip_prefix('+')
                    && let Some((_, bytes)) = current.as_mut()
                {
                    bytes.extend_from_slice(content.as_bytes());
                    bytes.push(b'\n');
                }
            }
            if let Some(target) = current {
                targets.push(target);
            }
        }
        "Bash" | "exec_command" | "functions.exec_command" | "shell" | "functions.shell" | "shell_command" | "functions.shell_command" | "local_shell" => {
            let command = fields
                .get("command")
                .and_then(Value::as_str)
                .or_else(|| fields.get("cmd").and_then(Value::as_str))
                .ok_or("missing command")?;
            targets = shell_targets(command, cwd)?;
        }
        _ => return Ok(None),
    }
    let code = home.join("code");
    for (path, content) in targets {
        let path = resolve(&path, cwd, home)?;
        if !path.starts_with(&code) || path.try_exists().map_err(|error| error.to_string())? {
            continue;
        }
        let Some(class) = classify(&path, &content) else {
            continue;
        };
        let repo = path
            .ancestors()
            .skip(1)
            .find(|ancestor| ancestor.join(".git").exists());
        let list = if let Some(repo) = repo {
            match fs::read(repo.join(ALLOWLIST)) {
                Ok(bytes) => Allowlist::parse(&bytes)?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => Allowlist::default(),
                Err(error) => return Err(error.to_string()),
            }
        } else {
            Allowlist::default()
        };
        let relative = repo
            .and_then(|repo| path.strip_prefix(repo).ok())
            .unwrap_or(&path);
        if class == Class::Shell
            || (class == Class::Script && !list.ratchet.contains(&Class::Script))
            || (class != Class::Python && list.allows(&relative.to_string_lossy(), class))
        {
            continue;
        }
        let reason = format!(
            "{}: new Python or Script file is forbidden; {ALTERNATIVE}; a Tom-approved exception belongs in {ALLOWLIST}",
            path.display()
        );
        return Ok(Some(
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":reason}}),
        ));
    }
    Ok(None)
}

pub fn run_guard() -> ExitCode {
    let mut input = Vec::new();
    let read = io::stdin().take(INPUT_LIMIT + 1).read_to_end(&mut input);
    let _ = io::copy(&mut io::stdin(), &mut io::sink());
    if read.is_ok()
        && let (Some(home), Ok(cwd)) = (std::env::var_os("HOME"), std::env::current_dir())
        && let Ok(Some(denial)) = guard_decision(
            &input,
            Path::new(&home),
            &cwd,
            std::env::var_os("AGENT_NO_AUTHORING_HOOKS").is_some(),
        ) {
            println!("{denial}");
        }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests;
