use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use north_base::build_inputs::runtime_input;
use north_base::platform::harness::{DEFAULTS, document};

use crate::land::candidate::git;
use crate::land::lander::{Lander, state_home};
use crate::land::queue::Ticket;

const UNIT: &str = "north-runtime-ahead";
const CANDIDATE: &str = "candidate";
const HAND_IN: &str = "hand-in";

fn requested() -> &'static Mutex<HashSet<String>> {
    static REQUESTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    REQUESTED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn considered() -> &'static Mutex<HashSet<String>> {
    static CONSIDERED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    CONSIDERED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Asks for the release north of the candidate the lander just built and will push, while its checks run: the landed commit carries this tree, so the promotion that follows the push finds the artifact ready and builds nothing (north#1022). The request runs as a transient unit of its own, so a lander restart neither waits on it nor kills it; the runtime has a build host build it, one at a time, and never builds it on this host.
pub(super) fn candidate(lander: &Lander, base: &str, head: &str) {
    if !changes_runtime(lander, base, head) {
        return;
    }
    ask(lander, head, CANDIDATE, "");
}

/// Asks, as the lander and never in a hand-in's own context, for the release north of each queued north lane's own tree that changes a runtime input: a lane that lands alone onto the main it was rebased on lands that very tree, so its build starts while the lane waits in the queue instead of seconds before its push; a lane that lands under another tree only costs a hand-in build, which yields to every candidate.
pub(super) fn queued(lander: &Lander, tickets: &[(PathBuf, Ticket)]) {
    if document(DEFAULTS)["build_ahead_at_hand_in"]["value"].as_bool() == Some(false) {
        return;
    }
    if let Ok(mut seen) = considered().lock() {
        seen.retain(|head| tickets.iter().any(|(_, ticket)| ticket.head == *head));
    }
    for (_, ticket) in tickets {
        if !considered().lock().is_ok_and(|mut seen| seen.insert(ticket.head.clone())) {
            continue;
        }
        let Ok(base) = git(&lander.worktree, &["merge-base", "refs/remotes/origin/main", &ticket.head]) else { continue };
        if changes_runtime(lander, &base, &ticket.head) {
            ask(lander, &ticket.head, HAND_IN, &ticket.branch);
        }
    }
}

fn ask(lander: &Lander, head: &str, kind: &str, lane: &str) {
    if north_base::platform::orchestration::read().is_ok_and(|cfg| !north_base::platform::orchestration::builds_ahead(&cfg, &north_base::platform::host::name())) {
        return;
    }
    let Ok(tree) = git(&lander.worktree, &["rev-parse", &format!("{head}^{{tree}}")]) else { return };
    let key = format!("{kind} {tree}");
    if ready(&tree) || !requested().lock().is_ok_and(|mut asked| asked.insert(key.clone())) {
        return;
    }
    if !request(lander, &tree, head, kind, lane) && let Ok(mut asked) = requested().lock() {
        asked.remove(&key);
    }
}

/// Every north candidate that changes a runtime input needs an artifact, `Live-fix:` or not: a coordinator's order of main's head otherwise spends most of its minutes building a commit that landed minutes earlier (north#1273); requests wait their turn on the build host, every candidate ahead of every hand-in, so the build of the tree about to land never waits behind a guess.
fn changes_runtime(lander: &Lander, base: &str, head: &str) -> bool {
    let north = git(&lander.repo, &["remote", "get-url", "origin"]).is_ok_and(|url| url.trim_end_matches('/').trim_end_matches(".git").rsplit(['/', ':']).next() == Some("north"));
    north && git(&lander.worktree, &["diff", "--name-only", base, head]).is_ok_and(|names| names.lines().any(runtime_input))
}

fn ready(tree: &str) -> bool {
    north_runtime_follow::layout::held(&state_home().join("north/runtime-artifacts").join(tree))
}

fn request(lander: &Lander, tree: &str, commit: &str, kind: &str, lane: &str) -> bool {
    let log = lander.queue.join("logs").join(format!("build-ahead-{}.log", &tree[..12]));
    let north = std::env::current_exe().unwrap_or_else(|_| north_base::platform::north_binary::north_binary());
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths([home.join(".local/bin"), home.join(".nix-profile/bin")].into_iter().chain(std::env::split_paths(&inherited))).unwrap_or(inherited);
    let _ = fs::create_dir_all(log.parent().unwrap_or(Path::new(".")));
    let mut unit = Command::new("systemd-run");
    unit.args(["--user", "--quiet", "--collect", "--slice=agent.slice", "--property=IOAccounting=yes", "--unit"]).arg(format!("{UNIT}-{kind}-{}", &tree[..12]));
    unit.arg(format!("--property=StandardOutput=append:{}", log.display())).arg(format!("--property=StandardError=append:{}", log.display()));
    unit.arg(format!("--setenv=HOME={}", home.display())).arg(format!("--setenv=PATH={}", path.to_string_lossy()));
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        unit.arg(format!("--setenv=XDG_STATE_HOME={}", PathBuf::from(state).display()));
    }
    unit.arg("--").arg(&north).args(["runtime", "build-ahead", "--tree", tree, "--commit", commit, "--kind", kind, "--lane", lane, "--checkout"]).arg(&lander.repo);
    unit.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let started = unit.status().is_ok_and(|status| status.success());
    match started {
        true => println!("north land: asked for the release north of tree {} while its checks run", &tree[..12]),
        false => println!("north land: could not ask for the release north of tree {} ahead; the promotion has a build host build it", &tree[..12]),
    }
    started
}
