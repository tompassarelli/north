//! `north ops strategy refusals`: launches placement refused in a window, the oracle of the `launch-refusals-per-hour` goal.
use std::collections::BTreeMap;
use std::path::Path;

use north_base::platform::decision_log;
use north_strategy_types::placement::{Case, Decision, Outcome};
use north_strategy_types::{Envelope, Kind};

const USAGE: &str = "usage: north ops strategy refusals [--since 1h]";
const DEFAULT_SINCE: f64 = 3600.0;

/// Launches whose last placement in the window refused them; a launch that waited out a saturated host and was then placed is not one.
pub fn refused_launches(root: &Path, since: f64, now: f64) -> usize {
    let mut last: BTreeMap<String, (f64, bool)> = BTreeMap::new();
    for (_, line) in decision_log::lines(root, Kind::Placement, "jsonl", since, now) {
        let Ok(logged) = serde_json::from_str::<Envelope<Case, Decision>>(&line) else { continue };
        if logged.at < since {
            continue;
        }
        let refused = matches!(logged.decision.outcome, Outcome::Refused { .. });
        let entry = last.entry(logged.case.launch.name).or_insert((logged.at, refused));
        if logged.at >= entry.0 {
            *entry = (logged.at, refused);
        }
    }
    last.values().filter(|(_, refused)| *refused).count()
}

/// Prints the refused launches per hour of the window as one number.
pub fn run(arguments: &[String]) -> u8 {
    let since = match arguments {
        [] => DEFAULT_SINCE,
        [flag, span] if flag == "--since" => match decision_log::span_seconds(span) {
            Some(span) => span,
            None => return usage(),
        },
        _ => return usage(),
    };
    let records = north_base::platform::records_mirror::read("north ops strategy refusals");
    let Some(root) = decision_log::root_in(records.home()) else {
        eprintln!("north ops strategy refusals: the decision log is off (NORTH_DECISION_LOG is empty)");
        return 2;
    };
    let now = decision_log::now();
    println!("{:.2}", refused_launches(&root, now - since, now) as f64 * 3600.0 / since);
    0
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}
