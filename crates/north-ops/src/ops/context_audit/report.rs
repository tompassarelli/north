use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::scan::{Audit, KINDS, Role};

const TOP_SKILLS: usize = 10;

fn tokens(chars: usize) -> usize {
    chars / 4
}

fn share(part: usize, whole: usize) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

fn session_json(audit: &Audit) -> Value {
    let mut chars = Map::new();
    let mut estimated = Map::new();
    for kind in KINDS {
        let count = audit.chars.get(&kind).copied().unwrap_or(0);
        chars.insert(kind.name().to_owned(), json!(count));
        estimated.insert(kind.name().to_owned(), json!(tokens(count)));
    }
    chars.insert("total".to_owned(), json!(audit.total()));
    estimated.insert("total".to_owned(), json!(tokens(audit.total())));
    let grounding = &audit.grounding;
    json!({
        "session": audit.session,
        "path": audit.path,
        "role": audit.role.name(),
        "chars": chars,
        "tokens": estimated,
        "skills": audit.skills.iter().map(|(name, chars)| (name.clone(), json!({"chars": chars, "tokens": tokens(*chars)}))).collect::<Map<_, _>>(),
        "references": audit.references.iter().map(|(path, chars)| (path.clone(), json!({"chars": chars, "tokens": tokens(*chars)}))).collect::<Map<_, _>>(),
        "grounding": {
            "injections": grounding.injections,
            "notification_injections": grounding.notification_injections,
            "named_paths": grounding.named,
            "later_read_paths": grounding.read,
            "precision": share(grounding.read, grounding.named),
        },
        "oversized_lines": audit.oversized_lines,
    })
}

struct Totals {
    injections: usize,
    notification: usize,
    named: usize,
    read: usize,
}

fn totals(audits: &[Audit]) -> Totals {
    audits.iter().fold(Totals { injections: 0, notification: 0, named: 0, read: 0 }, |sum, audit| Totals {
        injections: sum.injections + audit.grounding.injections,
        notification: sum.notification + audit.grounding.notification_injections,
        named: sum.named + audit.grounding.named,
        read: sum.read + audit.grounding.read,
    })
}

pub fn json(audits: &[Audit], since: &str, startup_tokens: usize, elapsed: Duration) -> String {
    let sum = totals(audits);
    let report = json!({
        "since": since,
        "estimator": "chars/4",
        "startup_tokens_per_session": startup_tokens,
        "runtime_seconds": elapsed.as_secs_f64(),
        "sessions": audits.iter().map(session_json).collect::<Vec<_>>(),
        "grounding": {
            "injections": sum.injections,
            "notification_injections": sum.notification,
            "named_paths": sum.named,
            "later_read_paths": sum.read,
            "precision": share(sum.read, sum.named),
        },
    });
    serde_json::to_string_pretty(&report).unwrap_or_default()
}

pub fn percentile(sorted: &[usize], fraction: f64) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn column(audits: &[Audit], role: Role, value: impl Fn(&Audit) -> usize) -> Vec<usize> {
    let mut values: Vec<usize> = audits.iter().filter(|audit| audit.role == role).map(|audit| tokens(value(audit))).collect();
    values.sort_unstable();
    values
}

fn percent(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_owned(), |value| format!("{:.1}%", value * 100.0))
}

pub fn summary(audits: &[Audit], since: &str, startup_tokens: usize, elapsed: Duration) -> String {
    let leads = audits.iter().filter(|audit| audit.role == Role::Lead).count();
    let mut lines = vec![
        format!(
            "context audit, last {since}: {} sessions ({leads} leads, {} workers) in {:.1} s; tokens = chars/4; startup {startup_tokens} per session from policy-gate measure",
            audits.len(),
            audits.len() - leads,
            elapsed.as_secs_f64()
        ),
        format!("{:<18}{:>10}{:>10}{:>12}{:>12}", "tokens/session", "lead p50", "lead p90", "worker p50", "worker p90"),
    ];
    let rows = KINDS.iter().map(|kind| (kind.name(), Some(*kind))).chain([("total", None)]);
    for (name, kind) in rows {
        let value = |audit: &Audit| kind.map_or_else(|| audit.total(), |kind| audit.chars.get(&kind).copied().unwrap_or(0));
        let lead = column(audits, Role::Lead, value);
        let worker = column(audits, Role::Worker, value);
        lines.push(format!(
            "{name:<18}{:>10}{:>10}{:>12}{:>12}",
            percentile(&lead, 0.5),
            percentile(&lead, 0.9),
            percentile(&worker, 0.5),
            percentile(&worker, 0.9)
        ));
    }
    let mut skills: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for audit in audits {
        for (name, chars) in &audit.skills {
            let entry = skills.entry(name).or_default();
            entry.0 += chars;
            entry.1 += 1;
        }
    }
    let mut ranked: Vec<_> = skills.into_iter().collect();
    ranked.sort_by(|left, right| right.1.0.cmp(&left.1.0).then(left.0.cmp(right.0)));
    lines.push(format!("top {TOP_SKILLS} injected skills by total tokens:"));
    for (name, (chars, sessions)) in ranked.iter().take(TOP_SKILLS) {
        lines.push(format!("  {name:<28}{:>10} tokens in {sessions} sessions", tokens(*chars)));
    }
    let sum = totals(audits);
    lines.push(format!(
        "grounding precision: {} ({} of {} named paths later read, over {} injections)",
        percent(share(sum.read, sum.named)),
        sum.read,
        sum.named,
        sum.injections
    ));
    lines.push(format!(
        "grounding injections on background task notifications: {} of {} ({})",
        sum.notification,
        sum.injections,
        percent(share(sum.notification, sum.injections))
    ));
    lines.join("\n")
}
