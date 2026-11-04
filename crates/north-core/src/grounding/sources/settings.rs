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
pub fn iter_docs() -> Vec<Doc> {
    let at = Timestamp::now().unix_seconds_f64().floor();
    hook_docs(
        &env_path(
            "NORTH_GROUNDING_SETTINGS",
            home().join(".claude/settings.json"),
        ),
        at,
    )
}
