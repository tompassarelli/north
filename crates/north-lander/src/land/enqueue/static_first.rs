use std::path::Path;

use north_base::platform::harness::{DEFAULTS, document};

use super::refuse::{DiffOnly, refuse_diff_only};
use super::{Environment, Timings, checkout, default_branch, fetch_base, git, lander_ledger_at, queue, record_refused, repositories, setting, state_home};
use crate::land::scans::outgoing::denylist::Denylist;
use crate::land::slots::runner::{self, QueueRefusal, STOPPED, Stop};

pub type QueueChecks = fn(&Path, &str, &str, &Stop) -> Result<(), QueueRefusal>;

pub(super) fn enabled() -> bool {
    document(DEFAULTS)["static_gates_first"]["value"].as_str() != Some("off")
}

pub(super) fn settled<T>(rest: Result<T, String>, refusal: Option<&String>) -> Result<T, String> {
    match (rest, refusal) {
        (Err(stopped), Some(refusal)) if stopped == STOPPED => Err(refusal.clone()),
        (rest, _) => rest,
    }
}
pub(in crate::land) fn judge_static_first(root: &Path, base: &str, head: &str) -> Result<(), String> {
    if !enabled() {
        return Ok(());
    }
    let timings = Timings::start(0);
    let refused = run_diff_only(root, base, head, &timings).map(drop);
    if let Err(why) = &refused {
        record_refused(root, (&state_home(), &repositories::local_host()), &timings, why);
    }
    refused
}

fn run_diff_only(root: &Path, base: &str, head: &str, timings: &Timings) -> Result<Option<String>, String> {
    let (_, branch) = checkout(root)?;
    let origin = git(root, &["remote", "get-url", "--push", "origin"]).map_err(|_| "origin has no push URL".to_owned())?;
    let state_home = state_home();
    let (host, denylist) = (repositories::local_host(), Denylist::from_env());
    let environment = Environment { called_micros: 0, state_home: &state_home, host: &host, session: None, waits: false, denylist: &denylist, queue_checks: runner::judge_queue_checks_until };
    let lander_host = setting::landing(root, &format!("refs/remotes/origin/{}", default_branch(root)), "landing.lander-host");
    let ledger = lander_ledger_at(&lander_host, &queue::directory(&state_home, &origin), &environment);
    refuse_diff_only(DiffOnly { environment: &environment, timings, root, branch: &branch, base, head, ledger, record: false })
}

pub fn static_gates(arguments: &[String]) -> u8 {
    let repo = match arguments {
        [] => Path::new("."),
        [flag, dir] if flag == "--repo" => Path::new(dir),
        _ => {
            eprintln!("usage: north land static-gates [--repo DIR]");
            return 64;
        }
    };
    let timings = Timings::start(0);
    let outcome = checkout(repo).and_then(|(root, _)| {
        let (_, base) = fetch_base(&root, &default_branch(&root))?;
        let head = git(&root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
        run_diff_only(&root, &base, &head, &timings)
    });
    match outcome {
        Ok(_) => {
            println!("safe-push: no diff-only refusal for the lane's commits; {}; commit the lane's changes first, since only committed changes are judged", timings.line());
            0
        }
        Err(refusal) => {
            eprintln!("safe-push: ✗ {refusal}; NOT queued; {}", timings.line());
            1
        }
    }
}
