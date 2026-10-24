use super::*;
use std::collections::HashSet;
pub fn rephrasings(ranked: &[String]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for item in [
        ranked.iter().take(3).cloned().collect::<Vec<_>>(),
        ranked.iter().take(5).cloned().collect(),
        ranked.iter().skip(1).take(3).cloned().collect(),
    ] {
        if !item.is_empty() && !out.contains(&item) {
            out.push(item);
        }
    }
    out.truncate(3);
    out
}
pub fn tier_b(words: &[String], mut db: Option<&mut store::Sqlite>) -> (Vec<String>, Vec<String>) {
    let deadline = Timestamp::now().unix_seconds_f64() + 55.0;
    let mut searched = vec![
        "skills/policy/ledger/issues index".into(),
        "14-day history".into(),
    ];
    let mut ranked = words.to_vec();
    let mut counts = std::collections::HashMap::new();
    for word in words {
        let count = db
            .as_mut()
            .and_then(|d| {
                d.query(&format!(
                    "select count(*) n from docs where docs match {};",
                    store::quote(&store::match_term(word))
                ))
                .ok()
            })
            .and_then(|v| v.first().map(|r| num(r, "n")))
            .filter(|n| *n != 0.0)
            .unwrap_or(1e9);
        counts.insert(word, count);
    }
    ranked.sort_by(|a, b| counts[a].total_cmp(&counts[b]));
    let mut lines = Vec::new();
    let mut seen = HashSet::new();
    for (i, phrase) in rephrasings(&ranked).iter().enumerate() {
        if Timestamp::now().unix_seconds_f64() > deadline {
            break;
        }
        let mut args = vec![];
        if i > 0 {
            args.push("-u");
        }
        args.extend(["-m", "-n", "6", "--json", "-r", "user,assistant"]);
        args.extend(phrase.iter().map(String::as_str));
        let timeout = (deadline - Timestamp::now().unix_seconds_f64()).max(5.0).to_string();
        let mut timed = vec![timeout.as_str(), "convo"];
        timed.extend(args);
        let Ok(out) = command("timeout", &timed) else {
            continue;
        };
        let Ok(rows) =
            serde_json::from_str::<Vec<Value>>(if out.trim().is_empty() { "[]" } else { &out })
        else {
            continue;
        };
        searched.push(format!("convo '{}'", phrase.join(" ")));
        for row in rows {
            let key = format!("convo:{}", s(&row, "id"));
            if seen.contains(&key) || row["partial"] == true {
                continue;
            }
            seen.insert(key.clone());
            lines.push(format!(
                "- {key} ({} {}Z session {}): \"{}\"",
                s(&row, "role"),
                cut(&s(&row, "ts"), 16),
                cut(&s(&row, "session"), 8),
                cut(&flat(&s(&row, "snippet")), 180)
            ));
        }
    }
    searched.push(format!(
        "handoff names/headings in {}",
        display(&handoffs_dir())
    ));
    lines.extend(handoff_hits(&ranked, deadline));
    (lines, searched)
}
pub fn handoff_hits(words: &[String], deadline: f64) -> Vec<String> {
    if words.is_empty() {
        return vec![];
    }
    let Ok(entries) = fs::read_dir(handoffs_dir()) else {
        return vec![];
    };
    let mut scored = Vec::new();
    let need = if words.len() > 2 { 2.0 } else { 1.0 };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let mut score = words
            .iter()
            .filter(|w| name.to_lowercase().contains(w.as_str()))
            .count() as f64;
        if name.ends_with(".md") && Timestamp::now().unix_seconds_f64() < deadline
            && let Ok(f) = fs::File::open(&path) {
                let mut text = String::new();
                let _ = f.take(4096).read_to_string(&mut text);
                let heads = text
                    .lines()
                    .filter(|l| l.starts_with('#'))
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase();
                score += 0.5 * words.iter().filter(|w| heads.contains(w.as_str())).count() as f64;
            }
        if score >= need && path.exists() {
            scored.push((score, mtime(&path), path));
        }
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.total_cmp(&a.1)));
    scored
        .into_iter()
        .take(6)
        .map(|(_, at, p)| format!("- {} (handoff, {})", display(&p), Timestamp::from_unix_seconds(at as i64).day_and_minute()))
        .collect()
}
