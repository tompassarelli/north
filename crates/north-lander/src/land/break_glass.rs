use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use super::after::{self, LandedLane};
use super::candidate::{self, Pick};
use super::lane_gates;
use super::incidents::{self, Noticed};
use super::lander::Lander;
use super::push_refusal::Refusal;
use super::queue::Ticket;
use super::scans::outgoing;
use super::serve::{Pushed, Serve};
use super::telemetry::Stages;
use super::watch;
use super::withdraw::find;

pub const USAGE: &str = "usage: north land break-glass TICKET --reason TEXT --approval GATE [--repo DIR] [--app-token PROGRAM]";
pub const CLASS: &str = "lander-break-glass";
const WORKTREE: &str = "lander-break-glass";

struct Options {
    ticket: String,
    reason: String,
    approval: i64,
    repo: PathBuf,
    app_token: Option<String>,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let mut options = Options { ticket: String::new(), reason: String::new(), approval: 0, repo: PathBuf::from("."), app_token: None };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--reason" => options.reason.clone_from(rest.next()?),
            "--approval" => options.approval = rest.next()?.trim_start_matches('#').parse().ok()?,
            "--repo" => options.repo = PathBuf::from(rest.next()?),
            "--app-token" => options.app_token = Some(rest.next()?.clone()),
            word if word.starts_with('-') || !options.ticket.is_empty() => return None,
            word => options.ticket = word.to_owned(),
        }
    }
    (!options.ticket.is_empty() && !options.reason.trim().is_empty() && options.approval > 0).then_some(options)
}

#[derive(Deserialize)]
struct Gate {
    id: i64,
    question: String,
    options: Vec<String>,
    answer: Option<String>,
    answered_by: Option<String>,
}

fn gates() -> Result<Vec<Gate>, String> {
    let output = Command::new(after::follow::program()).args(["gate", "list", "--json"]).stdin(Stdio::null()).stderr(Stdio::inherit()).output().map_err(|error| format!("north gate list: {error}"))?;
    if !output.status.success() {
        return Err(format!("north gate list exited {}", output.status));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("north gate list printed no gates: {error}"))
}

fn approved(gates: &[Gate], id: i64, branch: &str) -> Result<String, String> {
    let refused = |why: String| Err(format!("no recorded approval from Tom: {why}"));
    let Some(gate) = gates.iter().find(|gate| gate.id == id) else { return refused(format!("there is no gate {id}")) };
    if gate.answered_by.as_deref() != Some("tom") {
        return refused(format!("gate {id} was answered by {}, not by Tom", gate.answered_by.as_deref().unwrap_or("nobody yet")));
    }
    if !gate.question.contains("break-glass") || !gate.question.contains(branch) {
        return refused(format!("gate {id} does not ask to break glass for {branch}"));
    }
    match (gate.answer.as_deref(), gate.options.first()) {
        (Some(answer), Some(approve)) if answer.starts_with(approve.as_str()) => Ok(answer.to_owned()),
        (answer, _) => refused(format!("Tom answered gate {id} with {:?}, not its first option", answer.unwrap_or_default())),
    }
}

fn refusal(picks: &[Pick], branch: &str) -> Option<String> {
    match picks.first() {
        Some(Pick::Included) => None,
        Some(Pick::Decided(verdict) | Pick::Undecided(verdict) | Pick::Deferred(verdict) | Pick::Refused(lane_gates::Refused { verdict, .. })) => Some(format!("{branch}: {verdict}")),
        None => Some(format!("{branch}: no pick")),
    }
}

fn land(lander: &Lander, path: &Path, ticket: &Ticket, reason: &str) -> Result<String, String> {
    let base = candidate::prepare(&lander.repo, &lander.worktree)?;
    let built = candidate::build(&lander.worktree, &base, &[(path.to_path_buf(), ticket.clone())], &[], 1, &lander.queue.join("logs"))?;
    if let Some(reason) = refusal(&built.picks, &ticket.branch) {
        return Err(format!("{reason}; it does not land on main {base}, and nothing was pushed"));
    }
    let head = built.head.as_str();
    if outgoing::publication(&lander.worktree, &base, head).is_err() {
        return Err(format!("the secret scan or the privacy judge refused {base}..{head}; nothing pushed"));
    }
    let serve = Serve::new(lander);
    let token = serve.push_token(&base)?;
    let mut stages = Stages::start();
    stages.pushing();
    match serve.push(head, &base, token.as_deref())? {
        Pushed::Landed => {}
        Pushed::Moved => return Err(format!("main moved from {base} before the push; nothing pushed, run it again")),
        Pushed::Refused(Refusal::Terminal(line) | Refusal::Retry(line)) => return Err(format!("origin refused the push of {head}: {line}")),
    }
    stages.mark("push");
    let tip = built.tips.first().cloned().unwrap_or_else(|| head.to_owned());
    let detail = format!("break-glass push of {} as {tip} onto {base}: {reason}", ticket.branch);
    incidents::record(&lander.queue, path, ticket, Some(Noticed { class: CLASS, key: format!("{CLASS}:{}@{tip}", ticket.branch), noticed_by: "tom", detail: &detail }));
    if let Err(error) = watch::hand_off(lander, &base, head, vec![LandedLane { ticket: ticket.clone(), tip: tip.clone() }]) {
        println!("north land: could not hand the after-landing run of {head} to the lander, which starts it on its next restart: {error}");
    }
    after::finish(lander, &lander.worktree.join("intent"), head, &base, &stages, vec![(path.to_path_buf(), ticket.clone(), tip.clone())])?;
    Ok(tip)
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let lander = match Lander::new(&options.repo, vec!["safe-push".to_owned()]).and_then(|lander| Ok(Lander { worktree: candidate::lander_worktree(&options.repo, WORKTREE)?, app_token: options.app_token.clone().map_or(lander.app_token.clone(), |program| vec![program]), ..lander })) {
        Ok(lander) => lander,
        Err(error) => {
            eprintln!("north land break-glass: {error}");
            return 1;
        }
    };
    let outcome = find(&lander.queue, &options.ticket).and_then(|(path, ticket)| {
        let answer = approved(&gates()?, options.approval, &ticket.branch)?;
        println!("north land: breaking glass for {} under gate {}, answered by Tom: {answer}", ticket.branch, options.approval);
        land(&lander, &path, &ticket, options.reason.trim()).map(|tip| (ticket, tip))
    });
    match outcome {
        Ok((ticket, tip)) => {
            println!("north land: {} landed as {tip} by break-glass; its after-landing run and the sheriff's revert stay in force", ticket.branch);
            0
        }
        Err(error) => {
            eprintln!("north land break-glass: {error}");
            1
        }
    }
}
