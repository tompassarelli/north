use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use north_base::build_inputs::{live_fix, runtime_input};
use north_base::platform::store_refs::RUNTIME_PROGRAMS;

use crate::land::candidate::git;
use crate::land::lander::{Lander, state_home};

const UNIT: &str = "north-runtime-ahead";

fn requested() -> &'static Mutex<HashSet<String>> {
    static REQUESTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    REQUESTED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Asks for the release north of the candidate the lander just built and will push, while its checks run: the landed commit carries this tree, so the promotion that follows the push finds the artifact ready and builds nothing (north#1022). The request runs as a transient unit of its own, so a lander restart neither waits on it nor kills it; the runtime has a build host build it, one at a time, and never builds it on this host.
pub(super) fn candidate(lander: &Lander, base: &str, head: &str) {
    if !changes_runtime(lander, base, head) || !fixes_live(lander, base, head) {
        return;
    }
    let Ok(tree) = git(&lander.worktree, &["rev-parse", &format!("{head}^{{tree}}")]) else { return };
    if ready(&tree) || !requested().lock().is_ok_and(|mut asked| asked.insert(tree.clone())) {
        return;
    }
    if !request(lander, &tree, head) && let Ok(mut asked) = requested().lock() {
        asked.remove(&tree);
    }
}

/// Only north's own candidates that change a runtime input and carry `Live-fix:` need an artifact: they are promoted at once, so their promotion waits on a build; any other landing waits for its slot, which builds its head once, or reuses the live runtime.
fn changes_runtime(lander: &Lander, base: &str, head: &str) -> bool {
    let north = git(&lander.repo, &["remote", "get-url", "origin"]).is_ok_and(|url| url.trim_end_matches('/').trim_end_matches(".git").rsplit(['/', ':']).next() == Some("north"));
    north && git(&lander.worktree, &["diff", "--name-only", base, head]).is_ok_and(|names| names.lines().any(runtime_input))
}

fn fixes_live(lander: &Lander, base: &str, head: &str) -> bool {
    git(&lander.worktree, &["log", "--format=%B", &format!("{base}..{head}")]).is_ok_and(|messages| live_fix(&messages).is_some())
}

fn ready(tree: &str) -> bool {
    let artifact = state_home().join("north/runtime-artifacts").join(tree);
    RUNTIME_PROGRAMS.iter().all(|program| artifact.join(program).is_file())
}

fn request(lander: &Lander, tree: &str, commit: &str) -> bool {
    let log = lander.queue.join("logs").join(format!("build-ahead-{}.log", &tree[..12]));
    let north = std::env::current_exe().unwrap_or_else(|_| north_base::platform::north_binary::north_binary());
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths([home.join(".local/bin"), home.join(".nix-profile/bin")].into_iter().chain(std::env::split_paths(&inherited))).unwrap_or(inherited);
    let _ = fs::create_dir_all(log.parent().unwrap_or(Path::new(".")));
    let mut unit = Command::new("systemd-run");
    unit.args(["--user", "--quiet", "--collect", "--unit"]).arg(format!("{UNIT}-{}", &tree[..12]));
    unit.arg(format!("--property=StandardOutput=append:{}", log.display())).arg(format!("--property=StandardError=append:{}", log.display()));
    unit.arg(format!("--setenv=HOME={}", home.display())).arg(format!("--setenv=PATH={}", path.to_string_lossy()));
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        unit.arg(format!("--setenv=XDG_STATE_HOME={}", PathBuf::from(state).display()));
    }
    unit.arg("--").arg(&north).args(["runtime", "build-ahead", "--tree", tree, "--commit", commit, "--checkout"]).arg(&lander.repo);
    unit.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let started = unit.status().is_ok_and(|status| status.success());
    match started {
        true => println!("north land: asked for the release north of tree {} while its checks run", &tree[..12]),
        false => println!("north land: could not ask for the release north of tree {} ahead; the promotion has a build host build it", &tree[..12]),
    }
    started
}
