use std::path::PathBuf;

pub fn activation_paths(get: &impl Fn(&str) -> Option<String>) -> (PathBuf, PathBuf) {
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
