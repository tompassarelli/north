use std::path::PathBuf;

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

pub(crate) fn activation_paths(get: &impl Fn(&str) -> Option<String>) -> (PathBuf, PathBuf) {
    let root = get("NORTH_AGENT_STATE_ROOT").unwrap_or_else(|| {
        format!(
            "{}/.local/state/north/agents",
            get("HOME").unwrap_or_default()
        )
    });
    let activation = get("NORTH_AGENT_ACTIVATION");
    let active = get("NORTH_AGENT_ACTIVE");
    let list = active
        .clone()
        .or_else(|| {
            activation
                .as_ref()
                .map(|s| format!("{}.active", s.strip_suffix(".json").unwrap_or(s)))
        })
        .unwrap_or_else(|| format!("{root}/current/activation.active"));
    let json = activation
        .or_else(|| active.map(|s| format!("{}.json", s.strip_suffix(".active").unwrap_or(&s))))
        .unwrap_or_else(|| format!("{root}/current/activation.json"));
    (list.into(), json.into())
}

fn digest(value: &serde_json::Value) -> bool {
    value
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

fn guards_off(id: &str, get: &impl Fn(&str) -> Option<String>) -> bool {
    if let Some(value) = get("AGENT_NO_AUTHORING_HOOKS").filter(|s| !s.is_empty()) {
        return !matches!(value.as_str(), "0" | "false");
    }
    let id = id.strip_suffix(".sh").unwrap_or(id);
    let id = id.strip_suffix(".js").unwrap_or(id);
    let (list, path) = activation_paths(get);
    if list.is_file()
        && let Ok(contents) = std::fs::read(&list) {
            let wanted = format!("hook {id}");
            return !contents
                .split(|b| *b == b'\n')
                .any(|line| line.iter().copied().filter(|b| *b != 0).eq(wanted.bytes()));
        }
    let Some(data) = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    else {
        return true;
    };
    if data["schema"] != "north.agent-activation/v1"
        || !digest(&data["catalogDigest"])
        || !digest(&data["generationId"])
    {
        return true;
    }
    let Some(units) = data["units"].as_array() else {
        return true;
    };
    let matches: Vec<_> = units
        .iter()
        .filter(|u| u["kind"] == "hook" && u["id"] == id)
        .collect();
    matches.len() != 1 || matches[0]["permission"] != "on" || matches[0]["active"] != true
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_GUARDS_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn authoring_guards_off(id: &str) -> bool {
    #[cfg(test)]
    if TEST_GUARDS_OFF.with(|off| off.get()) {
        return true;
    }
    guards_off_in(id, &nonempty_env)
}

pub fn guards_off_in(id: &str, get: &impl Fn(&str) -> Option<String>) -> bool {
    guards_off(&get("NORTH_HOOK_ID").unwrap_or_else(|| id.to_owned()), get)
}

#[cfg(test)]
#[path = "activation_tests.rs"]
mod tests;
