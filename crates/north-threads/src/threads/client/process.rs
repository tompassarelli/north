use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::error::NorthResult;
use crate::platform::process::child_within;
use crate::threads::outside;

pub(super) enum Input {
    Inherit,
    Null,
    Bytes(Vec<u8>),
    File(File),
}

pub(super) fn agent_event_checks(argv: &[String]) -> Result<(), String> {
    let option = |name: &str| {
        let mut found = None;
        let prefix = format!("{name}=");
        for (index, arg) in argv.iter().enumerate() {
            if arg == name {
                found = argv.get(index + 1).cloned();
            } else if let Some(value) = arg.strip_prefix(&prefix) {
                found = Some(value.to_owned());
            }
        }
        found
    };
    if let Some(body) = option("--body") {
        outside::betterleaks_clean(&body)?;
    }
    if option("--key").is_some_and(|key| key.trim().is_empty()) {
        return Err("key must be nonempty text".into());
    }
    Ok(())
}

/// Starts `program` with `arguments` in its own process group, in its own systemd scope where a user manager runs, and
/// reaps it on a thread, so the caller neither waits on it nor leaves a zombie, and a oneshot caller that ends kills only its own cgroup.
pub(super) fn detach(program: &[OsString], arguments: &[&std::ffi::OsStr]) {
    let mut command = if own_scope() {
        let mut scoped = Command::new("systemd-run");
        scoped.args(["--user", "--scope", "--quiet", "--collect", "--"]).args(program);
        scoped
    } else {
        let mut plain = Command::new(&program[0]);
        plain.args(&program[1..]);
        plain
    };
    command.args(arguments).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);
    if let Ok(mut child) = command.spawn() {
        std::thread::spawn(move || child.wait());
    }
}

/// Whether `systemd-run --user --scope` works here: a user manager answers on this host.
fn own_scope() -> bool {
    Command::new("systemd-run").args(["--user", "--scope", "--quiet", "--collect", "true"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

fn feed(mut child: Child, bytes: Option<Vec<u8>>) -> Child {
    if let (Some(bytes), Some(mut stdin)) = (bytes, child.stdin.take()) {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }
    child
}

pub(super) fn execute(argv: &[OsString], input: Input, stdout: Stdio, wait: Duration) -> NorthResult<Option<ExitStatus>> {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).stdout(stdout);
    let bytes = match input {
        Input::Inherit => None,
        Input::Null => {
            command.stdin(Stdio::null());
            None
        }
        Input::File(file) => {
            command.stdin(file);
            None
        }
        Input::Bytes(bytes) => {
            command.stdin(Stdio::piped());
            Some(bytes)
        }
    };
    let mut child = feed(command.spawn()?, bytes);
    Ok(child_within(&mut child, wait)?)
}

pub(super) fn execute_capturing_stderr(argv: &[OsString], input: Input, stdout: Stdio, wait: Duration) -> NorthResult<(Option<ExitStatus>, String)> {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).stdout(stdout).stderr(Stdio::piped());
    let bytes = match input {
        Input::Inherit => None,
        Input::Null => {
            command.stdin(Stdio::null());
            None
        }
        Input::File(file) => {
            command.stdin(file);
            None
        }
        Input::Bytes(bytes) => {
            command.stdin(Stdio::piped());
            Some(bytes)
        }
    };
    let mut child = feed(command.spawn()?, bytes);
    let stderr = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        })
    });
    let status = child_within(&mut child, wait)?;
    let text = stderr.and_then(|handle| handle.join().ok()).unwrap_or_default();
    Ok((status, text))
}

pub(super) fn code_of(status: ExitStatus) -> i32 {
    status.code().or_else(|| status.signal().map(|signal| -signal)).unwrap_or(1)
}

pub(super) fn has_usage(data: &[u8]) -> NorthResult<bool> {
    for line in data.split(|byte| *byte == b'\n') {
        if line.trim_ascii().is_empty() {
            continue;
        }
        let row: Value = serde_json::from_slice(line)?;
        if row.get("type").and_then(Value::as_str) == Some("usage") {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn shell_quote(word: &str) -> String {
    if word.is_empty() {
        return "''".into();
    }
    if word.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&byte)) {
        return word.into();
    }
    format!("'{}'", word.replace('\'', "'\"'\"'"))
}

pub(super) fn python_json(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| {
        let mut text = String::from("\"");
        for unit in item.encode_utf16() {
            match unit {
                0x22 => text.push_str("\\\""),
                0x5c => text.push_str("\\\\"),
                0x0a => text.push_str("\\n"),
                0x0d => text.push_str("\\r"),
                0x09 => text.push_str("\\t"),
                0x08 => text.push_str("\\b"),
                0x0c => text.push_str("\\f"),
                0x20..=0x7e => text.push(unit as u8 as char),
                _ => text.push_str(&format!("\\u{unit:04x}")),
            }
        }
        text.push('"');
        text
    }).collect();
    format!("[{}]", quoted.join(", "))
}
