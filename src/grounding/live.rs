use super::*;
pub fn usage(message: &str) -> bool {
    re(r"(?i)\b(status report|executive|usage|pace|pacing|burn|quota|limits?|accounts?|claude ?[abc]|claude:[abc]|codex|5h|weekly|week|resets?|capacity|percent)\b|\d+ ?%").is_match(message)
}
pub fn org_query(message: &str) -> bool {
    re(r"(?i)\b(leads?|org|status report|executive|who is|staff|staffed|proxy)\b").is_match(message)
}
pub fn pace_lines() -> Vec<String> {
    let path = env_path(
        "NORTH_GROUNDING_PACE",
        home().join(".local/state/agents/pace-control.json"),
    );
    let Ok(data) = serde_json::from_str::<Value>(&read(&path)) else {
        return vec![];
    };
    let mut lines = vec![format!(
        "Usage, source of truth {} (written by north harness/lib/pace_control.py; never quote a lead's status line):",
        display(&path)
    )];
    if let Some(accounts) = data["accounts"].as_object() {
        for (name, account) in accounts {
            let windows = account["windows"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|w| {
                    format!(
                        "{} {:.0}% (resets {}Z, as of {}Z)",
                        s(w, "window"),
                        num(w, "used_pct").round(),
                        slice(&s(w, "resets_at"), 11, 5),
                        slice(&s(w, "as_of"), 11, 5)
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            lines.push(format!(
                "- {name}: {}; allowed_workers {}{}",
                if windows.is_empty() {
                    "no windows"
                } else {
                    &windows
                },
                account
                    .get("allowed_workers")
                    .map(Value::to_string)
                    .unwrap_or_else(|| "undefined".into()),
                if account["held"] == true { " held" } else { "" }
            ));
        }
    }
    lines
}
pub fn org_lines(_message: &str) -> Vec<String> {
    let north = std::env::var_os("NORTH_BIN").unwrap_or_else(|| "north".into());
    let Ok(output) = std::process::Command::new(north).args(["fleet", "tree"]).output() else {
        return vec![];
    };
    if !output.status.success() {
        return vec![];
    }
    let mut lines = vec!["Fleet sessions (north fleet tree):".to_owned()];
    lines.extend(String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned));
    let mut budget = 600usize;
    lines
        .into_iter()
        .filter_map(|l| {
            let compact = cut(&l, budget.saturating_sub(1));
            budget = budget.saturating_sub(ulen(&compact) + 1);
            (!compact.is_empty()).then_some(compact)
        })
        .collect()
}
