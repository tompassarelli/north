use crate::foundation::clock::Timestamp;
use crate::hook::{HookInput, Verdict};
use crate::platform::pyjson;
use crate::shell;
use serde_json::Value;
use std::collections::BTreeMap;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DENIAL: &str = "No. Tom is using the desktop. Use the private desktop, wait until he is idle, or put the need in your brief and announce it in one line first.";
const CONTEXT: &str = "desktop action guard: wrapped the desktop command to read the focused window when it runs and refocus that window on exit.";
const WRAPPERS: &[&str] = &[
    "env", "command", "exec", "sudo", "doas", "nohup", "setsid", "timeout", "nice", "stdbuf",
    "xargs", "!", "{", "if", "then", "elif", "while", "until", "do",
];

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}
fn uid() -> u32 {
    rustix::process::getuid().as_raw()
}
fn runtime() -> PathBuf {
    let dir = env("XDG_RUNTIME_DIR");
    if dir.is_empty() {
        format!("/run/user/{}", uid()).into()
    } else {
        dir.into()
    }
}
fn active() -> bool {
    let path = if !env("NORTH_AGENT_ACTIVATION").is_empty() {
        env("NORTH_AGENT_ACTIVATION")
    } else if !env("NORTH_AGENT_ACTIVE").is_empty() {
        format!(
            "{}.json",
            env("NORTH_AGENT_ACTIVE").trim_end_matches(".active")
        )
    } else {
        let root = env("NORTH_AGENT_STATE_ROOT");
        format!(
            "{}/current/activation.json",
            if root.is_empty() {
                format!("{}/.local/state/north/agents", env("HOME"))
            } else {
                root
            }
        )
    };
    let Some(data) = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return false;
    };
    if data["schema"] != "north.agent-activation/v1"
        || !["catalogDigest", "generationId"].iter().all(|key| {
            data[*key]
                .as_str()
                .and_then(|s| s.strip_prefix("sha256:"))
                .is_some_and(|s| {
                    s.len() == 64
                        && s.bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
        })
    {
        return false;
    }
    let Some(units) = data["units"].as_array() else {
        return false;
    };
    let mut matches = units
        .iter()
        .filter(|unit| unit["kind"] == "hook" && unit["id"] == "north-hook");
    matches
        .next()
        .is_some_and(|unit| unit["permission"] == "on" && unit["active"] == true)
        && matches.next().is_none()
}
// run-bounded gives a PID namespace over the host /proc, so getppid() cannot be looked up in /proc.
fn parent_of_self() -> Option<u32> {
    Some(stat_fields("/proc/self/stat")?.0)
}
fn stat_fields(path: &str) -> Option<(u32, String)> {
    let stat = std::fs::read_to_string(path).ok()?;
    let fields: Vec<_> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    Some((fields.get(1)?.parse().ok()?, fields.get(19)?.to_string()))
}
fn process(pid: u32) -> Option<(u32, String, String)> {
    let (parent, started) = stat_fields(&format!("/proc/{pid}/stat"))?;
    Some((
        parent,
        started,
        std::fs::canonicalize(format!("/proc/{pid}/exe"))
            .ok()?
            .file_name()?
            .to_str()?
            .to_owned(),
    ))
}
pub fn record_idle(idle: bool) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let mut pid = parent_of_self().unwrap_or(1);
    while pid > 1 {
        let (parent, started, name) =
            process(pid).ok_or_else(|| std::io::Error::other("cannot read callback ancestry"))?;
        if name == "swayidle" {
            let path = runtime().join("desktop-idle.json");
            let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
            std::fs::write(
                &temporary,
                serde_json::to_vec(&serde_json::json!({
                    "pid": pid, "started": started, "idle": idle
                }))?,
            )?;
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
            std::fs::rename(temporary, path)?;
            let state = env("XDG_STATE_HOME");
            let state = if state.is_empty() {
                PathBuf::from(env("HOME")).join(".local/state")
            } else {
                PathBuf::from(state)
            };
            let history = state.join("agents/presence/idle.jsonl");
            std::fs::create_dir_all(history.parent().unwrap())?;
            let at = Timestamp::now().iso_micros_utc_offset();
            let mut stream = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(history)?;
            writeln!(stream, "{}", serde_json::json!({"at": at, "idle": idle}))?;
            return Ok(());
        }
        pid = parent;
    }
    Err(std::io::Error::other("not a swayidle callback"))
}
fn seat_idle() -> bool {
    let Some(data) = std::fs::read(runtime().join("desktop-idle.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
    else {
        return false;
    };
    let pid = data["pid"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .or_else(|| data["pid"].as_str()?.parse().ok());
    pid.and_then(process).is_some_and(|(_, started, name)| {
        name == "swayidle" && data["started"] == started && data["idle"] == true
    })
}
fn value(assignments: &BTreeMap<String, String>, key: &str) -> String {
    assignments.get(key).cloned().unwrap_or_else(|| env(key))
}
fn private_target(assignments: &BTreeMap<String, String>, name: &str) -> bool {
    let runtime = value(assignments, "XDG_RUNTIME_DIR");
    let path = Path::new(&runtime);
    let Some(run) = path.parent() else {
        return false;
    };
    let root = env("NORTH_PRIVATE_DESKTOP_ROOT");
    let root = if root.is_empty() {
        format!("/run/user/{}", uid())
    } else {
        root
    };
    let display = value(assignments, "WAYLAND_DISPLAY");
    let socket = |p: &Path| p.metadata().is_ok_and(|m| m.file_type().is_socket());
    let valid = path.file_name().is_some_and(|n| n == "runtime")
        && run.parent() == Some(Path::new(&root))
        && run
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("private-desktop."))
        && run.metadata().is_ok_and(|m| m.uid() == uid())
        && run.join("active").is_file()
        && run.join("lifecycle-owned").is_file()
        && std::fs::read_to_string(run.join("wayland-display")).is_ok_and(|s| s.trim() == display)
        && !display.contains('/')
        && socket(&path.join(&display));
    if name == "niri" {
        let ipc = PathBuf::from(value(assignments, "NIRI_SOCKET"));
        valid && ipc.parent() == Some(path) && socket(&ipc)
    } else {
        valid && value(assignments, "WAYLAND_SOCKET").is_empty()
    }
}
fn quiet_close(args: &[String], assignments: &BTreeMap<String, String>) -> bool {
    let mut window = None;
    for (i, arg) in args.iter().enumerate() {
        if arg == "--id" && args.get(i + 1).is_some() {
            window = args.get(i + 1).map(String::as_str);
        } else if let Some(id) = arg.strip_prefix("--id=") {
            window = Some(id);
        }
    }
    let Some(window) = window
        .filter(|s| !s.is_empty() && s.chars().all(char::is_numeric))
        .and_then(|s| s.parse::<u64>().ok())
    else {
        return false;
    };
    let Ok(mut output) = tempfile::tempfile() else {
        return false;
    };
    let Ok(stdout) = output.try_clone() else {
        return false;
    };
    let mut command = Command::new("niri");
    command
        .args(["msg", "-j", "windows"])
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::null());
    if let Some(ipc) = assignments.get("NIRI_SOCKET") {
        command.env("NIRI_SOCKET", ipc);
    }
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            _ if start.elapsed() >= Duration::from_secs(5) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            _ => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    use std::io::{Read, Seek};
    let mut bytes = Vec::new();
    if output.rewind().is_err() || output.read_to_end(&mut bytes).is_err() {
        return false;
    }
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|v| {
            v.as_array().map(|items| {
                items.iter().any(|item| {
                    item.is_object()
                        && item["id"] == window
                        && item["app_id"] == "quiet.spawn"
                        && item["is_focused"] == false
                })
            })
        })
        .unwrap_or(false)
}
fn substitutions(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let tail = &text[cursor..];
        let Some(offset) = tail.find(['$', '`']) else {
            break;
        };
        cursor += offset;
        if text[cursor..].starts_with('`') {
            let start = cursor + 1;
            if let Some(end) = text[start..].find('`').filter(|end| *end > 0) {
                out.push(&text[start..start + end]);
            }
            cursor += 1;
        } else if text[cursor..].starts_with("$(") {
            let start = cursor + 2;
            let mut depth = 1;
            for (i, c) in text[start..].char_indices() {
                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                }
                if depth == 0 {
                    out.push(&text[start..start + i]);
                    break;
                }
            }
            cursor += 2;
        } else {
            cursor += 1;
        }
    }
    out
}
fn join(words: &[String]) -> String {
    words
        .iter()
        .map(|s| {
            if !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c))
            {
                s.clone()
            } else {
                format!("'{}'", s.replace('\'', "'\"'\"'"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn dangerous(text: &str, inherited: &BTreeMap<String, String>) -> bool {
    let stripped = shell::strip_heredocs(text);
    if substitutions(&stripped)
        .iter()
        .any(|body| *body != text && dangerous(body, inherited))
    {
        return true;
    }
    let tokens = shell::tokenize(&stripped, ";&|\n`", true, true, " \t\r");
    for part in tokens.split(|t| t.0 == "sep") {
        let words: Vec<_> = part.iter().map(|t| t.1.clone()).collect();
        let index = shell::command(&words, WRAPPERS);
        let Some(word) = words.get(index) else {
            continue;
        };
        let mut assignments = inherited.clone();
        for word in &words[..index] {
            if let Some((key, value)) = word.split_once('=')
                && !key.is_empty()
                    && key.chars().enumerate().all(|(i, c)| {
                        c.is_ascii_alphabetic() || c == '_' || (i > 0 && c.is_ascii_digit())
                    })
                {
                    assignments.insert(key.into(), value.into());
                }
        }
        let name = word.rsplit('/').next().unwrap_or("");
        let args = &words[index + 1..];
        if [
            "spawn-quiet",
            "automation-chrome",
            "private-desktop.sh",
        ]
        .contains(&name)
        {
            let nested = if name == "spawn-quiet" {
                args
            } else if name == "private-desktop.sh" && args.first().is_some_and(|s| s == "start") {
                args.iter()
                    .position(|s| s == "--")
                    .map(|i| &args[i + 1..])
                    .unwrap_or(&[])
            } else {
                &[]
            };
            if !nested.is_empty() && dangerous(&join(nested), &assignments) {
                return true;
            }
            continue;
        }
        if ["bash", "sh", "zsh", "dash"].contains(&name)
            && let Some(i) = args
                .iter()
                .position(|arg| arg.starts_with('-') && arg.contains('c') && !arg.starts_with("--"))
                && args.get(i + 1).is_some_and(|s| dangerous(s, &assignments)) {
                    return true;
                }
        if name == "eval" && dangerous(&args.join(" "), &assignments) {
            return true;
        }
        let action = if ["wtype", "ydotool"].contains(&name) {
            true
        } else if name == "niri" && args.iter().any(|s| s == "msg") {
            args.iter()
                .position(|s| s == "action")
                .and_then(|i| args.get(i + 1).map(|a| (i, a)))
                .is_some_and(|(i, a)| {
                    a.starts_with("focus-")
                        || a.starts_with("fullscreen-")
                        || a.starts_with("move-")
                        || (a == "close-window" && !quiet_close(&args[i + 2..], &assignments))
                })
        } else {
            false
        };
        if action && !(name != "ydotool" && private_target(&assignments, name)) {
            return true;
        }
    }
    false
}
fn restore_command(text: &str) -> String {
    format!(
        "(\n_north_desktop_focus=$(niri msg -j focused-window | jq -r '.id // empty') || exit $?\ncase \"$_north_desktop_focus\" in *[!0-9]*) exit 1;; esac\nreadonly _north_desktop_focus\n[ -z \"$_north_desktop_focus\" ] || trap 'niri msg action focus-window --id \"$_north_desktop_focus\" >/dev/null 2>&1 || :' EXIT\ntrap 'exit 130' INT\ntrap 'exit 143' TERM\n(\n{text}\n)\n)"
    )
}
pub fn decide(input: &HookInput) -> Verdict {
    if crate::hook::authoring_guards_off("north-hook")
        || !active()
        || !input.is_shell()
    {
        return Verdict::Allow;
    }
    let Some(text) = input.command() else {
        return Verdict::Allow;
    };
    let scan = text.lossy();
    if !dangerous(&scan, &BTreeMap::new()) {
        return Verdict::Allow;
    }
    if !seat_idle() {
        return Verdict::Deny(DENIAL.into());
    }
    let field = if input.tool_input["command"].is_null() { "cmd" } else { "command" };
    let mut updated = input.tool_input.clone();
    let wrapper = restore_command("\0");
    let (prefix, suffix) = wrapper.split_once('\0').unwrap();
    let parts = [
        pyjson::Value::from(prefix),
        pyjson::Value::String(text.clone()),
        pyjson::Value::from(suffix),
    ]
    .map(|value| pyjson::dumps(&value));
    let encoded = format!(
        "\"{}{}{}\"",
        &parts[0][1..parts[0].len() - 1],
        &parts[1][1..parts[1].len() - 1],
        &parts[2][1..parts[2].len() - 1]
    );
    let wrapped = pyjson::loads(encoded.as_bytes()).unwrap();
    updated[field] = if updated[field].as_array().is_some() {
        pyjson::Value::Array(vec!["bash".into(), "-lc".into(), wrapped])
    } else {
        wrapped
    };
    Verdict::Rewrite(updated, CONTEXT.into())
}
