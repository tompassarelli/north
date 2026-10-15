use super::*;
use std::collections::HashSet;
pub fn header_comment(path: &Path) -> String {
    let text = read(path);
    if cut(&text, 4096).contains('\0') {
        return String::new();
    }
    let mut out: Vec<String> = Vec::new();
    for line in text.split('\n').take(25) {
        if line.starts_with("#!") {
            continue;
        }
        if let Some(c) = re(r"^\s*(?:#|//)\s?(.*)$").captures(line) {
            out.push(re(r"^[=\-_]{4,}$").replace(&c[1], "").trim_end().into());
        } else {
            if !out.is_empty() || !line.trim().is_empty() {
                break;
            }
        }
    }
    out.iter()
        .enumerate()
        .filter(|(i, l)| !l.is_empty() || (*i > 0 && !out[*i - 1].is_empty()))
        .map(|(_, l)| l.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .into()
}
pub fn hook_docs(path: &Path, at: f64) -> Vec<Doc> {
    let Ok(settings) = serde_json::from_str::<Value>(&read(path)) else {
        return vec![];
    };
    let mut docs = Vec::new();
    let mut seen = HashSet::new();
    if let Some(hooks) = settings["hooks"].as_object() {
        for (event, registrations) in hooks {
            for registration in registrations.as_array().into_iter().flatten() {
                for hook in registration["hooks"].as_array().into_iter().flatten() {
                    let cmd = s(hook, "command");
                    if cmd.is_empty() {
                        continue;
                    }
                    let tokens: Vec<_> = cmd.split_whitespace().collect();
                    let file = tokens
                        .iter()
                        .rev()
                        .find(|t| t.starts_with('/'))
                        .or_else(|| tokens.last())
                        .copied()
                        .unwrap_or("");
                    let name = Path::new(file)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    let matcher = s(registration, "matcher");
                    let id = format!("hook:{event}:{matcher}:{name}");
                    if !seen.insert(id.clone()) {
                        continue;
                    }
                    let on = if matcher.is_empty() {
                        String::new()
                    } else {
                        format!(" on {matcher}")
                    };
                    let header = header_comment(Path::new(file));
                    let intro = format!(
                        "Claude Code settings.json registers the {name} hook ({file}) for the {event} event{}.",
                        if matcher.is_empty() {
                            ", every tool".into()
                        } else {
                            format!(", matcher {matcher}")
                        }
                    );
                    docs.push(doc(
                        "settings",
                        id,
                        format!(
                            ".claude/settings.json {event}{} {name}",
                            if matcher.is_empty() {
                                String::new()
                            } else {
                                format!(" {matcher}")
                            }
                        ),
                        format!("registered {event} hook {name}{on}"),
                        if header.is_empty() {
                            intro
                        } else {
                            format!("{intro}\n{header}")
                        },
                        at,
                        false,
                    ));
                }
            }
        }
    }
    docs
}
pub fn events_doc(path: &Path, at: f64) -> Option<Doc> {
    if !path.exists() {
        return None;
    }
    let source = read(path);
    let list = |name: &str| {
        let pattern = format!(r"(?m)^{name} = \(([^)]*)\)");
        re(&pattern)
            .captures(&source)
            .map(|c| {
                re(r"'([^']+)'")
                    .captures_iter(&c[1])
                    .map(|m| m[1].to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let kinds = list("KINDS");
    let fields = list("FIELDS");
    let mut lines=vec!["The durable event system (harness/lib/durable_events.py) stores supervisor events in the events table of the threads ledger SQLite database, ~/.local/state/threads/threads.db on the node that owns the ledger and snapshot.db on remote nodes; THREADS_DB overrides the path. Test fixtures name that file events.db.".into(),"Table events(seq, ts, item, kind, actor, tier, data): data is JSON carrying the event fields and delivery state.".into()];
    if !kinds.is_empty() {
        lines.push(format!("Event kinds: {}.", kinds.join(", ")));
    }
    if !fields.is_empty() {
        lines.push(format!("Event fields: {}.", fields.join(", ")));
    }
    lines.push("Appended through bin/threads and read by harness/lib/events.py.".into());
    Some(doc(
        "settings",
        "events-db".into(),
        "~/.local/state/threads/threads.db".into(),
        "durable events DB: the events table in the threads ledger threads.db".into(),
        lines.join("\n"),
        at,
        false,
    ))
}
pub fn iter_docs() -> Vec<Doc> {
    let at = Timestamp::now().unix_seconds_f64().floor();
    let mut out = hook_docs(
        &env_path(
            "NORTH_GROUNDING_SETTINGS",
            home().join(".claude/settings.json"),
        ),
        at,
    );
    let path = north_dir().join("harness/lib/durable_events.py");
    if let Some(d) = events_doc(&path, at) {
        out.push(d);
    }
    out
}
