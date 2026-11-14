use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::fleet::digest_report::{Facts, GateLine, render as digest_text};
use crate::fleet::runtime::now_ms;
use crate::fleet::store::SessionRow;
use crate::foundation::clock::Timestamp;
use crate::router::moment::Moment;

pub struct Work {
    pub text: String,
    pub json: Value,
}

const USAGE: &str = "usage: north work [--since DURATION] [--json]";

pub fn run(arguments: &[String]) -> u8 {
    let (mut window, mut json) = ("24h", false);
    let mut rest = arguments.iter();
    while let Some(word) = rest.next() {
        match word.as_str() {
            "--json" => json = true,
            "--since" => match rest.next() {
                Some(value) => window = value,
                None => {
                    eprintln!("{USAGE}");
                    return 2;
                }
            },
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    match super::Snapshot::new().work(window) {
        Ok(snapshot) => {
            let Some(work) = &snapshot.work else {
                eprintln!("north work: no work snapshot");
                return 1;
            };
            if json {
                println!("{}", work.json);
            } else {
                print!("{}", work.text);
            }
            0
        }
        Err(why) => {
            eprintln!("north work: {why}");
            1
        }
    }
}

pub fn read(window: &str, at: Timestamp) -> Result<Work, String> {
    let window_ms = super::duration_ms(window)?;
    let now = now_ms();
    let since = now.saturating_sub(window_ms);
    let fleet = crate::cli::fleet::open_fleet().map_err(|error| error.user_message())?;
    let books = fleet.digest(since).map_err(|error| error.user_message())?;
    let mut gates: Vec<GateLine> = books.open_gates().map(|gate| gate_line(&format!("{}:{}", fleet.host(), gate.id), gate.request.session.clone(), gate.request.question.clone(), recommended(&gate.request.options, gate.request.recommend), gate.default_at_ms())).collect();
    gates.extend(fleet.gates_elsewhere().map_err(|error| error.user_message())?.iter().map(|row| gate_line(&row.label(), row.request.session.clone(), row.request.question.clone(), recommended(&row.request.options, row.request.recommend), row.default_at_ms())));
    let sessions = fleet.cached_sessions().map_err(|error| error.user_message())?.unwrap_or_default();
    let held = fleet.held_since(since);
    let digest = digest_text(&Facts { now: Moment::now(), now_ms: now, since_label: window, landed: &books.landed, sessions: &sessions, gates: &gates, held: &held });
    let tree = tree(&sessions);
    let report = super::report::lines();
    let lanes = crate::flow::work_rows();
    let lane_lines = crate::flow::work_lines(&lanes);
    let text = text(window, &digest, &sessions, &tree, &report, &lane_lines);
    let json = json!({
        "at": at.iso(),
        "now_ms": now,
        "window": window,
        "landed": books.landed.iter().map(|landing| json!({"task": landing.task_ref, "sha": landing.sha, "landed_ms": landing.landed_ms})).collect::<Vec<_>>(),
        "sessions": sessions,
        "gates": gates.iter().map(|gate| json!({"label": gate.label, "session": gate.session, "question": gate.question, "recommended": gate.recommended, "default_at_ms": gate.default_at_ms})).collect::<Vec<_>>(),
        "held": held,
        "report": report,
        "lane_flow": lanes,
        "lane_lines": lane_lines,
    });
    Ok(Work { text, json })
}

fn gate_line(label: &str, session: String, question: String, recommended: String, default_at_ms: Option<u64>) -> GateLine {
    GateLine { label: label.to_owned(), session, question, recommended, default_at_ms }
}

fn recommended(options: &[String], number: usize) -> String {
    number.checked_sub(1).and_then(|index| options.get(index)).map_or_else(|| "none".to_owned(), |option| format!("{number}. {option}"))
}

fn text(window: &str, digest: &str, sessions: &[SessionRow], tree: &[String], report: &[String], lanes: &[String]) -> String {
    let mut out = format!("Work: last {window}\n\n{digest}\n");
    out.push_str(&format!("Session list: {}\n", sessions.len()));
    for row in sessions {
        let tasks = if row.tasks.is_empty() { "no task".to_owned() } else { row.tasks.join(", ") };
        out.push_str(&format!(
            "  {} [{}] {}/{} on {} since {}, updated {}, cwd {}, parent {}, placement {}: {tasks}\n",
            row.name,
            row.status,
            row.provider,
            row.account,
            row.host,
            row.created,
            row.updated,
            row.cwd,
            row.parent.as_deref().unwrap_or("-"),
            row.placement.as_deref().unwrap_or("-"),
        ));
    }
    out.push_str(&format!("\nSession tree: {}\n", tree.len()));
    for line in tree {
        out.push_str(&format!("  {line}\n"));
    }
    out.push_str("\nReport:\n");
    for line in report {
        out.push_str(&format!("  {line}\n"));
    }
    out.push_str("\nLane flow:\n");
    for line in lanes {
        out.push_str(&format!("  {line}\n"));
    }
    out
}

fn tree(rows: &[SessionRow]) -> Vec<String> {
    let known: BTreeSet<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    let mut lines = Vec::new();
    for row in rows.iter().filter(|row| row.parent.as_deref().is_none_or(|parent| !known.contains(parent))) {
        branch(rows, row, 0, &mut lines);
    }
    lines
}

fn branch(rows: &[SessionRow], row: &SessionRow, depth: usize, lines: &mut Vec<String>) {
    lines.push(format!("{}{} [{}] {}/{} on {}", "  ".repeat(depth), row.name, row.status, row.provider, row.account, row.host));
    for child in rows.iter().filter(|child| child.parent.as_deref() == Some(row.name.as_str())) {
        branch(rows, child, depth + 1, lines);
    }
}
