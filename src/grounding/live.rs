use super::*;
pub fn usage(message: &str) -> bool {
    re(r"(?i)\b(status report|executive|usage|pace|pacing|burn|quota|limits?|accounts?|claude ?[abc]|claude:[abc]|5h|weekly|week|resets?|capacity|percent)\b|\d+ ?%").is_match(message)
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
        "Usage, source of truth {} (written by north pace; never quote a lead's status line):",
        display(&path)
    )];
    if let Some(accounts) = data["accounts"].as_object() {
        for (name, account) in accounts {
            let windows = account["windows"]
                .as_array()
                .into_iter()
                .flatten()
                .map(window_line)
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
fn window_line(w: &Value) -> String {
    format!(
        "{} {:.0}% (resets {}, as of {}Z)",
        s(w, "window"),
        num(w, "used_pct").round(),
        reset(w),
        slice(&s(w, "as_of"), 11, 5)
    )
}
fn reset(w: &Value) -> String {
    let resets = s(w, "resets_at");
    let (Some(at), Some(now)) = (Timestamp::parse_loose(&resets), Timestamp::parse_loose(&s(w, "as_of"))) else {
        return format!("{}Z", slice(&resets, 11, 5));
    };
    let minutes = (at.unix_seconds() - now.unix_seconds()).max(0) / 60;
    let left = if minutes < 120 { format!("{minutes}m") } else { format!("{}h", (minutes + 30) / 60) };
    let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][at.unix_days().rem_euclid(7) as usize];
    format!("{weekday} {} {}Z, in {left}", at.iso_date(), slice(&at.iso_minute(), 11, 5))
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reset_names_day_date_and_time_left() {
        let week = json!({"window": "week", "used_pct": 99, "resets_at": "2026-10-17T12:00:00Z", "as_of": "2026-10-16T08:54:03Z"});
        assert_eq!(window_line(&week), "week 99% (resets Sat 2026-10-17 12:00Z, in 27h, as of 08:54Z)");
        let five = json!({"window": "5h", "used_pct": 100, "resets_at": "2026-10-16T09:20:00Z", "as_of": "2026-10-16T08:57:00Z"});
        assert_eq!(window_line(&five), "5h 100% (resets Fri 2026-10-16 09:20Z, in 23m, as of 08:57Z)");
    }
}
