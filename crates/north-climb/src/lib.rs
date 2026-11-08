//! `north ops climb`: north improves its own staffing policy from the worker ledger (docs/rationale/policy-climber.md).
//! A dry run names the week's largest measured loss, the candidate policy change a generator writes for
//! it, and the candidate's score from strategy replay against the current policy.
use north_base::platform::decision_log;
use north_dispatch::dispatch::{ARM_WINDOW, LEDGER_WINDOW};
use north_dispatch::ledger;

pub mod arms;
pub mod candidate;
pub mod lineage;
pub mod loss;
pub mod scope;
pub mod reward;
pub mod score;
#[cfg(test)]
mod scope_tests;
#[cfg(test)]
mod lineage_tests;

use candidate::Cheapest;
use lineage::{Entry, Status};
use north_graph::Graph;

const USAGE: &str = "usage: north ops climb run --dry-run [--since 7d]   the week's largest measured loss, one candidate for it, its replay score and the live arms";
const DEFAULT_REPLAY: &str = "7d";
const MIN_CLOSED: usize = 5;

pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let since = match words[..] {
        ["run", "--dry-run"] => DEFAULT_REPLAY,
        ["run", "--dry-run", "--since", span] => span,
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let Some(span) = decision_log::span_seconds(since) else {
        eprintln!("{USAGE}");
        return 2;
    };
    match dry_run(span) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("north ops climb: {error}");
            1
        }
    }
}

fn dry_run(span: f64) -> Result<(), String> {
    let report = ledger::closed_report(LEDGER_WINDOW).map_err(|error| error.user_message())?;
    println!("north ops climb run --dry-run: ledger {LEDGER_WINDOW}, {} closed boxes, ${:.2} spent", report.closed, report.spend_usd);
    let Some(worst) = loss::largest(&report) else {
        println!("largest loss: none, no cell spent on a repeat run or an unclosed box");
        return Ok(());
    };
    println!(
        "largest loss: {} on {}: ${:.2} (repeat runs ${:.2}, unclosed boxes ${:.2}) over {} closed boxes in {} runs",
        worst.category,
        worst.tier,
        worst.usd(),
        worst.repeat_usd,
        worst.unclosed_usd,
        worst.closed,
        worst.runs
    );
    let armed = ledger::closed_report(ARM_WINDOW).map_err(|error| error.user_message())?;
    match (Cheapest { min_closed: MIN_CLOSED }).propose(&report, &worst) {
        Some(candidate) => remember(candidate, &worst, &armed, span)?,
        None => println!("candidate: none"),
    }
    println!("live arms over {ARM_WINDOW}, the shares dispatch draws each launch from (kept boxes split by who ticked them, rework judge v{}; only checker-verified boxes win a selection):", armed.judge);
    for arm in arms::live(&armed) {
        let cost = arm.usd_per_kept().map_or("no kept box".to_owned(), |usd| format!("${usd:.2} per kept box"));
        println!("  {} {}: share {:.0}% of its {} arms ({}), {} boxes, {} closed, {} kept ({} checker-verified, {} self-ticked), {cost}, issue-level read {} checker-verified", arm.category, arm.arm, 100.0 * arm.share, arm.family, if arm.settled { "settled by the bootstrap rule".to_owned() } else { format!("Thompson over {} seeded draws, floor {:.0}%", north_dispatch::router::settle::DRAWS, 100.0 * north_dispatch::router::settle::FLOOR) }, arm.boxes, arm.closed, arm.kept, arm.verified, arm.kept - arm.verified, arm.issue_verified);
    }
    Ok(())
}

fn show(candidate: &candidate::Candidate, span: f64) -> Result<Judged, String> {
    println!("candidate {} (parent {}): {}", candidate.id, candidate.parent, candidate.change.describe());
    let saving = candidate.saving_usd.map_or("projected saving unmeasured until the arm runs".to_owned(), |usd| format!("projected saving ${usd:.2}"));
    println!("  why: {}; {saving}", candidate.why);
    let touches = candidate.change.touches();
    let refused = scope::refusals(&touches, None);
    if !refused.is_empty() {
        refused.iter().for_each(|reason| println!("  refused: {reason}"));
        return Ok(Judged { status: Status::Refused, refusals: refused.len(), reached: 0 });
    }
    println!("  touches: {}; inside what a candidate may change", touches.join(", "));
    let root = decision_log::root().ok_or("the decision log is off (NORTH_DECISION_LOG is empty)")?;
    let scored = score::replay(&candidate.change, &root, span, decision_log::now());
    println!("  replay against the current policy: {} of the window's placement cases reached by the change", scored.reached);
    println!("  {}", scored.placement.replace('\n', "\n  "));
    match scored.refused_by_candidate.as_slice() {
        [] => println!("  score: passes, no outcome shifted or invariant breached beyond the {} the current policy already refuses", scored.refused_now),
        refusals => refusals.iter().for_each(|reason| println!("  score: refused, {reason}")),
    }
    let status = if scored.passes() { Status::Proposed } else { Status::Refused };
    Ok(Judged { status, refusals: scored.refused_by_candidate.len(), reached: scored.reached })
}

struct Judged {
    status: Status,
    refusals: usize,
    reached: usize,
}

/// Draws the candidate's parent across the archive, judges it, and tells it to the archive in one transaction at the graph's head.
fn remember(mut candidate: candidate::Candidate, loss: &loss::Loss, armed: &ledger::closed::ClosedReport, span: f64) -> Result<(), String> {
    let path = north_graph::default_path();
    let failed = |why: north_graph::GraphError| format!("the climb archive in {}: {why}", path.display());
    let mut graph = if path.is_file() { Some(Graph::open(&path).map_err(failed)?) } else { None };
    let seq = match &graph {
        Some(graph) => {
            let seq = graph.head().map_err(failed)?;
            let nodes = lineage::archive(graph).map_err(failed)?;
            candidate.parent = lineage::draw(&nodes, lineage::seed(&candidate.id, seq));
            println!("archive: {} climb nodes in {} at seq {seq}; parent {} drawn across them", nodes.len(), path.display(), candidate.parent);
            seq
        }
        None => {
            println!("archive: no graph at {}; parent {}", path.display(), candidate.parent);
            0
        }
    };
    let judged = show(&candidate, span)?;
    let Some(graph) = graph.as_mut() else { return Ok(()) };
    let reward = match candidate.change.arm() {
        Some(arm) => Some(lineage::reward(&armed.arms, &loss.category, arm, &lineage::corroborated(graph, seq).map_err(failed)?, armed.judge)),
        None => None,
    };
    let scored = reward.as_ref().map_or("no arm to score".to_owned(), |reward| format!("{} corroborated of {} kept boxes", reward.verified, reward.kept));
    let entry = Entry { candidate: &candidate, loss, status: judged.status, replay_refusals: judged.refusals, replay_reached: judged.reached, reward };
    let node = lineage::record(graph, &entry, seq).map_err(failed)?;
    println!("archive: told {node} under writer {}: status {}, parent {}, landed seq {seq}, {scored}", lineage::WRITER, judged.status.keyword(), candidate.parent);
    Ok(())
}
