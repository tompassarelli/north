//! `north climb`: north improves its own staffing policy from the worker ledger (docs/rationale/policy-climber.md).
//! A dry run names the week's largest measured loss, the candidate policy change a generator writes for
//! it, and the candidate's score from strategy replay against the current policy.
use north_base::platform::decision_log;
use north_dispatch::dispatch::LEDGER_WINDOW;
use north_dispatch::ledger;

pub mod arms;
pub mod candidate;
pub mod loss;
pub mod scope;
pub mod score;

use candidate::Cheapest;

const USAGE: &str = "usage: north climb run --dry-run [--since 7d]   the week's largest measured loss, one candidate for it, its replay score and the live arms";
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
            eprintln!("north climb: {error}");
            1
        }
    }
}

fn dry_run(span: f64) -> Result<(), String> {
    let report = ledger::closed_report(LEDGER_WINDOW).map_err(|error| error.user_message())?;
    println!("north climb run --dry-run: ledger {LEDGER_WINDOW}, {} closed boxes, ${:.2} spent", report.closed, report.spend_usd);
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
    match (Cheapest { min_closed: MIN_CLOSED }).propose(&report, &worst) {
        Some(candidate) => show(&candidate, span)?,
        None => println!("candidate: none"),
    }
    println!("live arms:");
    for arm in arms::live(&report) {
        let cost = arm.usd_per_kept().map_or("no kept box".to_owned(), |usd| format!("${usd:.2} per kept box"));
        println!("  {} {}: share {:.0}% (alternating), {} boxes, {} closed, {} kept, {cost}", arm.category, arm.arm, 100.0 * arm.share, arm.boxes, arm.closed, arm.kept);
    }
    Ok(())
}

fn show(candidate: &candidate::Candidate, span: f64) -> Result<(), String> {
    println!("candidate {} (parent {}): {}", candidate.id, candidate.parent, candidate.change.describe());
    let saving = candidate.saving_usd.map_or("projected saving unmeasured until the arm runs".to_owned(), |usd| format!("projected saving ${usd:.2}"));
    println!("  why: {}; {saving}", candidate.why);
    let touches = candidate.change.touches();
    let refused = scope::refusals(&touches);
    if !refused.is_empty() {
        refused.iter().for_each(|reason| println!("  refused: {reason}"));
        return Ok(());
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
    Ok(())
}
