use std::collections::{HashMap, HashSet};
use std::io::Write;

use super::{Ids, Options, merged};
use crate::providers::claude::runs::landing::MainIndex;
use crate::providers::claude::runs::telemetry::Prices;
use crate::providers::claude::runs::{CATALOG, DEFAULTS};
use crate::providers::ledger_rows::Shared;
use crate::providers::{agy, claude, codex, opencode};

/// This host's provider rows the ledger lacks.
pub(super) struct Rows {
    pub(super) rows: Vec<u8>,
    /// A provider's store went unread.
    pub(super) unread: bool,
    /// A config dir on this host holds transcripts no declared account reads.
    pub(super) missed: bool,
}

pub(super) fn rows(options: &Options, ids: &Ids) -> Option<Rows> {
    let recorded: HashSet<String> = settled_ids(&ids.recorded).into_keys().collect();
    let known = if options.backfill { HashMap::new() } else { settled_ids(&ids.settled) };
    let text = merged(&options.since);
    let mut rows = Vec::new();
    let session = Some(options.session.as_str()).filter(|session| !session.is_empty());
    let missed = match claude::runs::rows(session, &options.since, known.clone(), recorded.clone(), &text, &mut rows) {
        Ok(unread) if unread.is_empty() => false,
        Ok(unread) => {
            eprintln!("north ledger worker: {}", claude::runs::unread_error(&unread).user_message());
            true
        }
        Err(error) => {
            eprintln!("error: {}", error.user_message());
            return None;
        }
    };
    let known: HashSet<String> = known.into_keys().collect();
    let main = MainIndex::parse(&text);
    let prices = Prices::parse(&CATALOG, &DEFAULTS);
    let shared = Shared { session: &options.session, since: &options.since, known: &known, recorded: &recorded, main: &main, prices: &prices };
    let mut unread = false;
    for (name, outcome) in [("codex", codex::runs::rows(&shared, &mut rows)), ("agy", agy::rows(&shared, &mut rows))] {
        if let Err(error) = outcome {
            eprintln!("north ledger worker: {name} rows unread: {}", error.user_message());
            unread = true;
        }
    }
    if let Err(error) = deepseek_rows(&shared, &mut rows) {
        eprintln!("north ledger worker: deepseek rows unread: {}", error.user_message());
        unread = true;
    }
    capacity_rows(&ids.capacity, &mut rows);
    Some(Rows { rows, unread, missed })
}

/// This host's DeepSeek sessions, from the turns its session hosts recorded, the review verdicts and the push log.
fn deepseek_rows(shared: &Shared, rows: &mut Vec<u8>) -> crate::error::NorthResult<()> {
    let ledger = crate::ledger::Ledger::open_default()?;
    let (turns, prompts) = (ledger.runs()?, ledger.first_prompts()?);
    let mut reviewed: HashMap<String, Vec<String>> = HashMap::new();
    for verdict in crate::dispatch::deepseek::sampling::read(&crate::dispatch::launch_directory()) {
        reviewed.entry(verdict.session).or_default().push(verdict.sha);
    }
    let nodes = crate::providers::ledger_rows::fleet_nodes();
    let pushes = crate::fleet::landings::pushes(&crate::fleet::landings::push_log());
    let sources = opencode::runs::Sources { turns: &turns, prompts: &prompts, nodes: nodes.as_deref(), pushes: &pushes, reviewed: &reviewed };
    for row in opencode::runs::rows(shared, &sources, crate::providers::ledger_rows::cutoff(shared.since)?) {
        writeln!(rows, "{row}")?;
    }
    Ok(())
}

/// This host's capacity admissions the ledger lacks: those at or after its newest recorded one, the ledger ignoring a repeat.
fn capacity_rows(seen: &str, rows: &mut Vec<u8>) {
    let host = crate::platform::host::name();
    let since = seen.lines().filter_map(|line| line.split_once('\t')).find(|(name, _)| *name == host).map(|(_, at)| at);
    for row in crate::capacity::host::admission_ledger_rows(&host, since) {
        rows.extend_from_slice(format!("{row}\n").as_bytes());
    }
}

fn settled_ids(listed: &str) -> HashMap<String, Option<String>> {
    listed
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| match line.split_once('\t') {
            Some((id, ended)) => (id.to_owned(), Some(ended.to_owned())),
            None => (line.to_owned(), None),
        })
        .collect()
}

