use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::fleet::digest_report::{Facts, GateLine, render as digest_text};
use crate::fleet::runtime::now_ms;
use crate::fleet::store::SessionRow;
use crate::foundation::clock::Timestamp;
use crate::ledger::WorkerTurn;
use crate::router::moment::Moment;
use north_unblocks::review::{self, ClassLoss, Owners, classes, mirror_closed, threads_db};

mod landed;

pub struct Work {
    pub text: String,
    pub json: Value,
}

const USAGE: &str = "usage: north work [--since DURATION] [--json] | north work own CLASS --issue REPO#N";

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

/// `north work own CLASS --issue REPO#N`: records the open, owned issue that owns a harness-loss class (north#1577).
pub fn own(arguments: &[String]) -> u8 {
    let [class, flag, issue] = arguments else {
        eprintln!("{USAGE}");
        return 2;
    };
    if flag != "--issue" {
        eprintln!("{USAGE}");
        return 2;
    }
    let now = u64::try_from(Timestamp::now().unix_seconds()).unwrap_or_default();
    match Owners::here().record(class, issue, now, &north_unblocks::GitHub) {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err(why) => {
            eprintln!("north work own: {why}");
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
    let silent: Vec<String> = fleet.silent_sessions(std::time::SystemTime::now()).iter().map(|one| one.line()).collect();
    let digest = digest_text(&Facts { now: Moment::now(), now_ms: now, since_label: window, landed: &books.landed, sessions: &sessions, gates: &gates, held: &held, silent: &silent });
    let tree = tree(&sessions);
    let report = super::report::lines();
    let losses = harness_losses(window_ms, now);
    let today = landed::read(at);
    let headline = match &today {
        Ok(landed) => landed.lines(),
        Err(why) => format!("Landed today: unread: {why}\n"),
    };
    let turns = crate::ledger::worker_turns(window).map_err(|error| error.user_message());
    let round = crate::dispatch::round::last();
    let round_line = match &round {
        Ok(round) => round.line(),
        Err(why) => format!("Dispatch's last round unread: {why}"),
    };
    let text = format!("{headline}{round_line}\n\n{}{}", text(window, &digest, &sessions, &tree, &report, &losses), turn_section(&turns));
    let json = json!({
        "worker_turns": match &turns {
            Ok(found) => Value::Array(found.iter().map(|turn| json!({"agent": turn.agent, "provider": turn.provider, "started": turn.started, "minutes": turn.minutes, "tokens_in": turn.tokens_in, "tokens_out": turn.tokens_out, "outcome": turn.outcome, "landed": turn.landed, "landed_sha": turn.landed_sha})).collect()),
            Err(why) => json!({"unread": why}),
        },
        "at": at.iso(),
        "now_ms": now,
        "window": window,
        "landed_today": match &today {
            Ok(landed) => landed.json(),
            Err(why) => json!({"unread": why}),
        },
        "dispatch_round": match &round {
            Ok(round) => json!(round),
            Err(why) => json!({"unread": why}),
        },
        "landed": books.landed.iter().map(|landing| json!({"task": landing.task_ref, "sha": landing.sha, "landed_ms": landing.landed_ms})).collect::<Vec<_>>(),
        "sessions": sessions,
        "gates": gates.iter().map(|gate| json!({"label": gate.label, "session": gate.session, "question": gate.question, "recommended": gate.recommended, "default_at_ms": gate.default_at_ms})).collect::<Vec<_>>(),
        "held": held,
        "report": report,
        "harness_losses": match &losses {
            Ok(found) => Value::Array(found.iter().map(|class| json!({"class": class.class, "runs": class.runs, "usd": class.usd, "owner": class.owner})).collect()),
            Err(why) => json!({"unread": why}),
        },
    });
    Ok(Work { text, json })
}

fn gate_line(label: &str, session: String, question: String, recommended: String, default_at_ms: Option<u64>) -> GateLine {
    GateLine { label: label.to_owned(), session, question, recommended, default_at_ms }
}

fn recommended(options: &[String], number: usize) -> String {
    number.checked_sub(1).and_then(|index| options.get(index)).map_or_else(|| "none".to_owned(), |option| format!("{number}. {option}"))
}

fn turn_section(turns: &Result<Vec<WorkerTurn>, String>) -> String {
    let mut out = String::from("\nWorker turns:\n");
    match turns {
        Ok(found) if found.is_empty() => out.push_str("  none\n"),
        Ok(found) => found.iter().for_each(|turn| out.push_str(&format!("  {}\n", turn_line(turn)))),
        Err(why) => out.push_str(&format!("  unread: {why}\n")),
    }
    out
}

fn turn_line(turn: &WorkerTurn) -> String {
    let landed = if turn.landed_sha.is_empty() { "landed".to_owned() } else { format!("landed {}", turn.landed_sha) };
    let landed = if turn.landed { landed } else { "not landed".to_owned() };
    format!("{} [{}] started {}, {}m wall, tokens in {} out {}, exit {}, {landed}", turn.agent, turn.provider, turn.started, turn.minutes, turn.tokens_in, turn.tokens_out, turn.outcome)
}

fn text(window: &str, digest: &str, sessions: &[SessionRow], tree: &[String], report: &[String], losses: &Result<Vec<ClassLoss>, String>) -> String {
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
    out.push_str("\nHarness losses, costliest first:\n");
    match losses {
        Ok(found) if found.is_empty() => out.push_str("  none\n"),
        Ok(found) => found.iter().for_each(|class| out.push_str(&format!("  {}\n", class.line()))),
        Err(why) => out.push_str(&format!("  unread: {why}\n")),
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

/// The harness-loss classes of the window, each with the issue that owns it (north#1577).
fn harness_losses(window_ms: u64, now_ms: u64) -> Result<Vec<ClassLoss>, String> {
    let reading = review::since(&threads_db(), i64::try_from(now_ms.saturating_sub(window_ms) / 1000).unwrap_or_default())?;
    classes(&reading, &Owners::here(), &mirror_closed)
}
