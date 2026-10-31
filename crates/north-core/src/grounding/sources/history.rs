use super::*;
use std::collections::{HashMap, HashSet};
pub fn is_brief(text: &str) -> bool {
    let trimmed = text.trim_start();
    let harness=(trimmed.starts_with('<')&&!trimmed.starts_with("<pasted_content")&&!trimmed.starts_with("<north_machine_relay"))||re(r"^\s*(/|\[Request interrupted|\[routine:|Caveat:|This session is being continued|Stop hook feedback)").is_match(text);
    harness||re(r"(?i)^\s*(Delegation:|Read (and follow|\S+ and follow|(?:/home/tom|~|\$HOME)/\.local/state/agents/handoffs)|You are (a|an|the) |(LEAD )?COACH\b|Coach repl|\S+-lead -> |From (\S+-lead|the proxy)|\S+-lead box |Run exactly this |New priority:|Use (the Agent tool|spawn_agent|the automation-chrome)|(Review|Adversarial review) (this|of a) diff|─{5})").is_match(text)||cut(text,400).contains("Another Claude session sent a message")||re(r"(?m)^(Category|Scope|Surface|Verify|Spec|Parent|Setup|Done when):").find_iter(text).count()>=2
}
pub fn session_kind(opening: Option<&str>) -> &'static str {
    let Some(text) = opening else {
        return "tom";
    };
    if re(r"(?i)handoffs/proxy-handoff|take over as the proxy|You are the proxy")
        .is_match(&cut(text, 600))
    {
        "proxy"
    } else if is_brief(text) {
        "agent"
    } else {
        "tom"
    }
}
pub fn speaker(role: &str, kind: &str, text: &str) -> Option<&'static str> {
    if role == "user" {
        let relay=re(r"(from=proxy|^<north_machine_relay/>\s*(EVENT \d+ \w+\s+from=proxy|Proxy)|\bTom\b|\bTOM\b)").is_match(&cut(text,600));
        if re(r"^\s*(Claude[:,] |\S+-lead[,:] )").is_match(text) && relay {
            return Some("relay");
        }
        if re(r"^\s*<north_machine_relay").is_match(text) {
            return if relay && !is_brief(&re(r"^\s*<north_machine_relay/>\s*").replace(text, "")) {
                Some("relay")
            } else {
                None
            };
        }
        return if is_brief(text) || text.split_whitespace().count() < 2 {
            None
        } else {
            Some("Tom")
        };
    }
    if kind == "agent" {
        None
    } else if role == "assistant" && text.split_whitespace().count() >= 4 {
        Some("proxy")
    } else {
        None
    }
}
pub fn transcripts(since: Option<f64>, path: &Path) -> Result<Vec<Doc>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let mut db = Sqlite::open(path, true)?;
    db.exec("PRAGMA busy_timeout=10000;")?;
    let floor = since.unwrap_or(0.0).max(Timestamp::now().unix_seconds_f64() - 14.0 * 86400.0);
    let rows=db.query(&format!("select m.id, m.ts, m.role, m.file_id, f.session_id, b.text from msg m join files f on f.id=m.file_id join body b on b.id=m.body where m.ts > {} and f.agent is null and m.dup=0 and m.role in ('user','assistant') order by m.ts;",quote(&Timestamp::from_unix_seconds_f64(floor).iso_millis())))?;
    let mut kinds = HashMap::new();
    let mut out = Vec::new();
    for row in rows {
        let text = s(&row, "text");
        let role = s(&row, "role");
        if text.is_empty() {
            continue;
        }
        let fid = num(&row, "file_id") as i64;
        if let std::collections::hash_map::Entry::Vacant(e) = kinds.entry(fid) {
            let first=db.query(&format!("select b.text from msg m join body b on b.id=m.body where m.file_id={fid} and m.role='user' order by m.line limit 1;"))?;
            e.insert(session_kind(first.first().and_then(|r| r["text"].as_str())));
        }
        let Some(who) = speaker(&role, kinds[&fid], &text) else {
            continue;
        };
        let id = s(&row, "id");
        let at = epoch(&s(&row, "ts"));
        out.push(doc(
            "transcripts",
            id.clone(),
            format!("convo:{id}"),
            format!(
                "{who} {} session {}",
                Timestamp::from_unix_seconds(at as i64).day_and_minute(),
                cut(&s(&row, "session_id"), 8)
            ),
            cut(&text, 1500),
            at,
            true,
        ));
    }
    Ok(out)
}
pub fn builds(since: Option<f64>) -> Result<Vec<Doc>> {
    let path = convo_path();
    if !path.exists() {
        return Ok(vec![]);
    }
    let mut db = Sqlite::open(&path, true)?;
    let floor = since.unwrap_or(0.0).max(Timestamp::now().unix_seconds_f64() - 90.0 * 86400.0);
    let rows=db.query(&format!("select m.id,m.ts,f.project,b.text from msg m join files f on f.id=m.file_id join body b on b.id=m.body where m.ts>{} and m.role='assistant' and m.dup=0 and (b.text like 'Done%' or b.text like 'Not done%' or b.text like 'Blocked%' or b.text like '**Done%' or b.text like '%LANDED%');",quote(&Timestamp::from_unix_seconds_f64(floor).iso_millis())))?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let text = s(&row, "text");
            if text.is_empty()
                || !re(r"^\s*(\*\*)?(Done|Not done|Blocked)\b|\bLANDED\b")
                    .is_match(&cut(&text, 4000))
            {
                return None;
            }
            let at = epoch(&s(&row, "ts"));
            let id = s(&row, "id");
            Some(doc(
                "builds",
                id.clone(),
                format!("convo:{id}"),
                format!(
                    "report {} {}: {}",
                    cut(&Timestamp::from_unix_seconds(at as i64).day_and_minute(), 6),
                    s(&row, "project"),
                    cut(&flat(&text), 110)
                ),
                cut(&text, 1500),
                at,
                true,
            ))
        })
        .collect())
}
pub fn commits(since: Option<f64>, code: &Path) -> Result<Vec<Doc>> {
    let floor = since.unwrap_or(0.0).max(Timestamp::now().unix_seconds_f64() - 90.0 * 86400.0).floor();
    let mut seen = HashSet::new();
    let mut docs = Vec::new();
    for main in repos::owned_mains(code) {
        let repo = main
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy();
        let Ok(out) = command(
            "git",
            &[
                "-C",
                &main.to_string_lossy(),
                "log",
                &format!("--since=@{floor}"),
                "--format=\u{1e}%H %ct %s",
                "--name-only",
                "HEAD",
            ],
        ) else {
            continue;
        };
        for chunk in out.split('\u{1e}').skip(1) {
            let lines: Vec<_> = chunk.split('\n').filter(|l| !l.trim().is_empty()).collect();
            let Some(head) = lines.first() else {
                continue;
            };
            let parts: Vec<_> = head.split(' ').collect();
            if parts.len() < 2 {
                continue;
            }
            let sha = parts[0];
            let id = format!("{repo}@{sha}");
            if !seen.insert(id.clone()) {
                continue;
            }
            docs.push(doc(
                "commits",
                id,
                format!("{repo}@{}", cut(sha, 8)),
                format!("landed {repo}@{}: {}", cut(sha, 8), parts[2..].join(" ")),
                lines
                    .iter()
                    .skip(1)
                    .take(60)
                    .copied()
                    .collect::<Vec<_>>()
                    .join("\n"),
                parts[1].parse().unwrap_or(0.0),
                true,
            ));
        }
    }
    Ok(docs)
}
