use super::*;
use compose::{Rendered, Shape};
use std::collections::HashSet;

const SYSTEM_TAGS: [&str; 12] = [
    "task-notification",
    "system-reminder",
    "local-command-caveat",
    "local-command-stdout",
    "local-command-stderr",
    "command-name",
    "command-message",
    "command-args",
    "bash-input",
    "bash-stdout",
    "bash-stderr",
    "user-prompt-submit-hook",
];
const KEEP_DAYS: f64 = 14.0;

pub fn typed_text(prompt: &str) -> String {
    let mut text = prompt.to_owned();
    for tag in SYSTEM_TAGS {
        text = re(&format!(r"(?s)<{tag}>.*?</{tag}>")).replace_all(&text, "").into_owned();
        text = re(&format!(r"(?s)\A\s*<{tag}>.*\z")).replace_all(&text, "").into_owned();
    }
    text.trim().to_owned()
}

#[derive(Default, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub refs: Vec<String>,
    #[serde(default)]
    pub live: Option<String>,
}

fn session_path(id: &str) -> Option<PathBuf> {
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .then(|| state_dir().join("sessions").join(format!("{id}.json")))
}

pub fn load(id: &str) -> Session {
    session_path(id)
        .and_then(|p| serde_json::from_str(&read(&p)).ok())
        .unwrap_or_default()
}

pub fn save(id: &str, session: &Session) {
    let Some(path) = session_path(id) else { return };
    let Some(dir) = path.parent() else { return };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let now = Timestamp::now().unix_seconds_f64();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p != path && now - mtime(&p) > KEEP_DAYS * 86400.0 {
                let _ = fs::remove_file(p);
            }
        }
    }
    let tmp = path.with_extension("json.tmp");
    if let Ok(body) = serde_json::to_string(session)
        && fs::write(&tmp, body).is_ok()
    {
        let _ = fs::rename(tmp, path);
    }
}

pub fn render(prompt: &str, now: f64, prior: &str, session: &mut Session) -> Result<String> {
    let typed = typed_text(prompt);
    if typed.is_empty() {
        return Ok(String::new());
    }
    let seen: HashSet<String> = session.refs.iter().cloned().collect();
    let Rendered { text, refs, live } = compose::shaped(&typed, "A", now, prior, &Shape::hook(seen, session.live.clone()))?;
    if live.is_some() {
        session.live = live;
    }
    for r in refs {
        if !session.refs.contains(&r) {
            session.refs.push(r);
        }
    }
    Ok(text)
}

#[cfg(test)]
#[path = "hook_tests.rs"]
mod tests;
