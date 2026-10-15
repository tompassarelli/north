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
fn last_status(path: &str) -> (f64, String) {
    let path = Path::new(path);
    let at = mtime(path);
    (
        at,
        cut(
            tail(path, 4096)
                .trim()
                .split('\n')
                .next_back()
                .unwrap_or(""),
            160,
        ),
    )
}
pub fn org_lines(message: &str) -> Vec<String> {
    let path = env_path(
        "AGENTS_ORG_FILE",
        home().join(".local/state/agents/org.json"),
    );
    let Ok(data) = serde_json::from_str::<Value>(&read(&path)) else {
        return vec![];
    };
    let nodes: Vec<_> = data["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| n["state"] == "active")
        .collect();
    let low = message.to_lowercase();
    let mut chosen: Vec<_> = nodes
        .iter()
        .copied()
        .filter(|n| {
            let id = s(n, "id");
            low.contains(&id.to_lowercase())
                || low.contains(&id.trim_end_matches("-lead").replace('-', " "))
        })
        .collect();
    let mut lines = vec![format!(
        "Org (same as `agents org show`, from {}): {} active leads: {}",
        display(&path),
        nodes.len(),
        cut(
            &nodes
                .iter()
                .map(|n| format!("{} ({}, {})", s(n, "id"), s(n, "domain"), s(n, "provider")))
                .collect::<Vec<_>>()
                .join(", "),
            700
        )
    )];
    if chosen.is_empty() {
        chosen = nodes;
        chosen.sort_by(|a, b| {
            last_status(&s(b, "status_file"))
                .0
                .total_cmp(&last_status(&s(a, "status_file")).0)
        });
        chosen.truncate(3);
    }
    for node in chosen {
        let path = s(node, "status_file");
        let (at, line) = last_status(&path);
        if at != 0.0 {
            lines.push(format!(
                "- {} status {} @ {}: {line}",
                s(node, "id"),
                display(Path::new(&path)),
                stamp(at)
            ));
        }
    }
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
