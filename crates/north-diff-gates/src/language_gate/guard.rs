use super::allowlist::{Allowlist, Class, classify};
use super::{ALLOWLIST, ALTERNATIVE, INPUT_LIMIT, Result};
use crate::shell::{ShellToken, operator_tokens, strip_heredocs};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

pub(super) fn resolve(path: &str, cwd: &Path, home: &Path) -> Result<PathBuf> {
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

pub(super) fn anchored(dir: &Path, path: &str) -> String {
    if path.starts_with("~/") || path.starts_with("$HOME/") || path.starts_with("${HOME}/") {
        path.into()
    } else {
        dir.join(path).to_string_lossy().into_owned()
    }
}

pub(super) fn changed_directory(dir: &Path, operands: &[&str]) -> Option<PathBuf> {
    match operands.first().copied() {
        None | Some("~" | "$HOME" | "${HOME}") => Some(PathBuf::from("~/")),
        Some(target) if target.contains('$') && !target.starts_with("$HOME/") && !target.starts_with("${HOME}/") => None,
        Some(target) => Some(PathBuf::from(anchored(dir, target))),
    }
}

pub(super) fn is_name(name: &str) -> bool {
    name.chars().next().is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && name.chars().all(|part| part == '_' || part.is_ascii_alphanumeric())
}

pub(super) fn expanded(word: &str, assigned: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(word.len());
    let mut rest = word;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let (name, used) = match after.strip_prefix('{').and_then(|inner| inner.split_once('}')) {
            Some((name, _)) => (name, name.len() + 2),
            None => {
                let end = after.find(|part: char| part != '_' && !part.is_ascii_alphanumeric()).unwrap_or(after.len());
                (&after[..end], end)
            }
        };
        match assigned.get(name) {
            Some(value) if is_name(name) => out.push_str(value),
            _ => out.push_str(&rest[at..at + 1 + used]),
        }
        rest = &after[used..];
    }
    out.push_str(rest);
    out
}

pub(super) fn shell_targets(command: &str, cwd: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let command = strip_heredocs(command);
    let tokens = operator_tokens(&command)?;
    let mut targets = Vec::new();
    let mut dir = cwd.to_path_buf();
    let mut scopes = Vec::new();
    let mut owned: Vec<String> = Vec::new();
    let mut assigned: HashMap<String, String> = HashMap::new();
    let mut redirect: Option<&str> = None;
    for token in tokens
        .iter()
        .chain(std::iter::once(&ShellToken::Operator(";".into())))
    {
        match token {
            ShellToken::Word(word) if redirect.is_some() => {
                if matches!(redirect, Some(">" | ">>")) {
                    targets.push((anchored(&dir, &expanded(word, &assigned)), Vec::new()));
                }
                redirect = None;
            }
            ShellToken::Word(word) => owned.push(expanded(word, &assigned)),
            ShellToken::Operator(operator)
                if matches!(
                    operator.as_str(),
                    ";" | "&&" | "||" | "|" | "\n" | "(" | ")" | "&"
                ) =>
            {
                let words: Vec<&str> = owned.iter().map(String::as_str).collect();
                let start = words
                    .iter()
                    .position(|word| !word.contains('='))
                    .unwrap_or(words.len());
                if start == words.len() {
                    assigned.extend(words.iter().filter_map(|word| word.split_once('=')).filter(|(name, _)| is_name(name)).map(|(name, value)| (name.to_owned(), value.to_owned())));
                }
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
                        "cd" => {
                            if let Some(next) = changed_directory(&dir, &operands) {
                                dir = next;
                            }
                        }
                        "touch" | "tee" => {
                            for path in operands {
                                targets.push((anchored(&dir, path), Vec::new()));
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
                                            anchored(&dir, &Path::new(target).join(name).to_string_lossy()),
                                            fs::read(dir.join(source)).unwrap_or_default(),
                                        ));
                                    }
                                }
                            } else if let Some(target) = operands.last() {
                                if dir.join(target).is_dir() {
                                    for source in operands.iter().take(operands.len() - 1) {
                                        if let Some(name) = Path::new(source).file_name() {
                                            targets.push((
                                                anchored(&dir, &Path::new(target).join(name).to_string_lossy()),
                                                fs::read(dir.join(source)).unwrap_or_default(),
                                            ));
                                        }
                                    }
                                } else {
                                    let content = operands
                                        .first()
                                        .and_then(|source| fs::read(dir.join(source)).ok())
                                        .unwrap_or_default();
                                    targets.push((anchored(&dir, target), content));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                match operator.as_str() {
                    "(" => scopes.push(dir.clone()),
                    ")" => {
                        if let Some(outer) = scopes.pop() {
                            dir = outer;
                        }
                    }
                    _ => {}
                }
                owned.clear();
                redirect = None;
            }
            ShellToken::Operator(operator)
                if matches!(operator.as_str(), ">" | ">>" | "<" | "<<") =>
            {
                redirect = Some(operator.as_str())
            }
            _ => {}
        }
    }
    Ok(targets)
}

pub(super) fn command_text(fields: &Value) -> Option<String> {
    let command = fields.get("command").or_else(|| fields.get("cmd"))?;
    if let Some(text) = command.as_str() {
        return Some(text.into());
    }
    let argv: Vec<&str> = command.as_array()?.iter().filter_map(Value::as_str).collect();
    match argv.as_slice() {
        [shell, flag, script, ..]
            if Path::new(shell).file_name().is_some_and(|name| {
                matches!(name.to_str(), Some("bash" | "sh" | "zsh" | "dash" | "fish"))
            }) && flag.starts_with('-')
                && flag.ends_with('c') =>
        {
            Some((*script).into())
        }
        [] => None,
        _ => Some(
            argv.iter()
                .map(|word| format!("'{}'", word.replace('\'', "'\\''")))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    }
}

pub(super) fn patch_envelope(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) if text.contains("*** Begin Patch") => Some(text),
        Value::Object(fields) => fields.values().find_map(patch_envelope),
        _ => None,
    }
}

pub(super) fn guard_decision(input: &[u8], home: &Path, cwd: &Path, disabled: bool) -> Result<Option<Value>> {
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
            let command = command_text(fields).ok_or("missing command")?;
            targets = shell_targets(&command, cwd)?;
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
