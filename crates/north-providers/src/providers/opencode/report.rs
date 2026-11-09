use std::collections::{BTreeMap, BTreeSet};

use north_base::land_ticket::{self, Phase};

use crate::foundation::clock::Timestamp;
use crate::ledger::{Ledger, Run, Task};

const USAGE: &str = "usage: north ledger opencode [--since YYYY-MM-DD]";

const DEEPSEEK: [&str; 2] = ["opencode", "pi"];

#[derive(Default)]
struct Tally {
    landed: usize,
    rejected: usize,
    pending: usize,
}

struct Session {
    arm: String,
    tasks: BTreeSet<String>,
}

pub fn run(arguments: &[String]) -> u8 {
    let mut since = None;
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--since" => match words.next().and_then(|date| Timestamp::parse_loose(date)).map(Timestamp::unix_millis) {
                Some(at) => since = Some(at),
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
    let ledger = match Ledger::open_default() {
        Ok(ledger) => ledger,
        Err(error) => {
            eprintln!("north ledger opencode: {}", error.user_message());
            return 1;
        }
    };
    let (runs, tasks) = match (ledger.runs(), ledger.tasks()) {
        (Ok(runs), Ok(tasks)) => (runs, tasks),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("north ledger opencode: {}", error.user_message());
            return 1;
        }
    };
    let tasks: BTreeMap<&str, &Task> = tasks.iter().map(|task| (task.task_ref.as_str(), task)).collect();
    let lanes = land_ticket::lanes_by_session(&crate::platform::land_queue::queues(&crate::platform::records_mirror::state_home())).unwrap_or_default();
    print(&tally(&runs, &tasks, &lanes, since));
    0
}

fn tally(runs: &[Run], tasks: &BTreeMap<&str, &Task>, lanes: &BTreeMap<String, Vec<land_ticket::Lane>>, since: Option<i64>) -> BTreeMap<String, Tally> {
    let mut sessions: BTreeMap<&str, Session> = BTreeMap::new();
    for run in runs.iter().filter(|run| deepseek(run) && since.is_none_or(|since| run.started_ms >= since)) {
        let session = sessions.entry(run.session.as_str()).or_insert_with(|| Session { arm: run.overlay.clone().unwrap_or_else(|| "unlabelled".into()), tasks: BTreeSet::new() });
        if let Some(task) = &run.task_ref {
            session.tasks.insert(task.clone());
        }
    }
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    for (name, session) in sessions {
        let tally = tallies.entry(session.arm).or_default();
        let landed = session.tasks.iter().filter_map(|task| tasks.get(task.as_str())).any(|task| task.outcome.as_deref() == Some("landed") && task.landed_sha.as_deref().is_some_and(|sha| !sha.is_empty()))
            || lanes.get(name).is_some_and(|lanes| lanes.iter().any(|lane| lane.phase == Phase::Landed));
        let rejected = !landed
            && (session.tasks.iter().filter_map(|task| tasks.get(task.as_str())).any(|task| matches!(task.outcome.as_deref(), Some("abandoned" | "not done")))
                || lanes.get(name).is_some_and(|lanes| lanes.iter().any(|lane| matches!(lane.phase, Phase::Failed | Phase::Conflict | Phase::Quarantined | Phase::Withdrawn))));
        match (landed, rejected) {
            (true, _) => tally.landed += 1,
            (false, true) => tally.rejected += 1,
            (false, false) => tally.pending += 1,
        }
    }
    tallies
}

fn deepseek(run: &Run) -> bool {
    DEEPSEEK.contains(&run.provider.as_str()) || run.model.as_deref().is_some_and(|model| model.contains("deepseek"))
}

fn print(tallies: &BTreeMap<String, Tally>) {
    println!("opencode overlay accept rate:");
    println!("{:<20} {:>7} {:>9} {:>8} {:>8}", "arm", "landed", "rejected", "accept", "pending");
    for (arm, tally) in tallies {
        let decided = tally.landed + tally.rejected;
        let accept = if decided == 0 { "-".to_owned() } else { format!("{:.1}%", 100.0 * tally.landed as f64 / decided as f64) };
        println!("{arm:<20} {:>7} {:>9} {:>8} {:>8}", tally.landed, tally.rejected, accept, tally.pending);
    }
}
