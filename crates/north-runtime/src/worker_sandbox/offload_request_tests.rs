//! The cases of the process the outside broker starts for a worker command's offload request (south#118). The test that runs them starts itself again as a broker with a home, a `PATH` and a host declaration of its own, and that broker starts it once more as the `north` of each request, which records what it was started with and, for `free` and the `choose` that leaves this host out, runs the offload verb itself.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::foundation::offload::{LANDER_SLOT_ENV, OWNER_ENV, RED_FIX_ENV};

use super::super::broker::{self, Request};
use super::super::lane_grant::Host;
use super::super::{Context, removable};
use super::{OPEN_SCOPE, git};
use crate::foundation::shim::write_script;

const ROLE: &str = "NORTH_118_OFFLOAD_ROLE";
const SCRATCH: &str = "NORTH_118_OFFLOAD_SCRATCH";
const WORDS: &str = "NORTH_118_OFFLOAD_WORDS";
const TEST: &str = "worker_sandbox::tests::the_outside_broker_runs_only_north_s_fixed_verbs_with_an_environment_that_carries_no_injected_code";
const BUILD_HOST: &str = "n118-build";
const HERE: &str = "n118-here";
const CLAIMS: [&str; 3] = [OWNER_ENV, RED_FIX_ENV, LANDER_SLOT_ENV];
const LAND_CHECK_CEILING: &str = "2100";
const GIT_PLACES: [&str; 3] = ["GIT_DIR", "GIT_COMMON_DIR", "GIT_WORK_TREE"];
const FOREIGN: &str = "names another repository";
const REWRITE: &str = "rewrite";
const OWN_PATH: [&str; 3] = ["lane sync", "home move", "threads list"];
const SERVED: [&str; 14] = [
    "safe-push --to main",
    "task comment x",
    "lane sync /c/main",
    "lane unlock x",
    "lane stray y",
    "lane remove z",
    "lane gc",
    "depbox run -- true",
    "depbox fetch -- true",
    "capacity host run --class moderate --owner o --timeout-seconds 9 -- true",
    "home move plan.tsv",
    "threads x",
    "--worker-sandbox --built -- ./t",
    "--worker-sandbox --lane -- /bin/sh -c true",
];

/// Whether this process is one the test started again, as the broker or as a request's `north`; it has then played its part.
pub(super) fn played() -> bool {
    let Some(root) = std::env::var_os(SCRATCH).map(PathBuf::from) else { return false };
    match std::env::var(ROLE).as_deref() {
        Ok("broker") => serve(&root),
        Ok("north") => offload(&root),
        _ => return false,
    }
    true
}

pub(super) fn an_offload_request_s_process_takes_nothing_of_the_worker_s_but_its_typed_words() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().canonicalize().unwrap();
    let home = root.join("home");
    let main = home.join("code/alpha/main");
    let policy = home.join("code/south/main/agents");
    for directory in [main.clone(), policy.clone(), home.join("code/alpha/worktrees"), root.join("elsewhere"), root.join("trusted"), root.join("fakes"), root.join("runtime")] {
        std::fs::create_dir_all(directory).unwrap();
    }
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "one"]);
    git(&main, &["worktree", "add", "-q", "../worktrees/lane", "-b", "lane"]);
    git(&root.join("elsewhere"), &["init", "-q", "-b", "main"]);
    std::fs::write(policy.join("orchestration.toml"), format!("[hosts.{HERE}]\nreaches = [\"{BUILD_HOST}\"]\n\n[hosts.{BUILD_HOST}]\nssh = \"{BUILD_HOST}.invalid\"\n")).unwrap();
    let path = std::env::var("PATH").unwrap();
    let real_git = std::env::split_paths(&path).map(|directory| directory.join("git")).find(|git| git.is_file()).unwrap();
    let ran = root.join("ran");
    write_script(&root.join("trusted/ssh"), &format!("echo 'trusted ssh' >> {}\ncase \"$*\" in *land-waiting*) echo \"$*\" >> {} ;; esac\necho 3\n", ran.display(), root.join("claimed").display()));
    write_script(&root.join("fakes/ssh"), &format!("echo 'fake ssh' >> {}\necho 3\n", ran.display()));
    write_script(&root.join("fakes/git"), &format!("echo 'fake git' >> {}\nexec {} \"$@\"\n", ran.display(), real_git.display()));
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env_clear()
        .env("PATH", format!("{}:{path}", root.join("trusted").display()))
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("RESIDENT_CAP_HOST", HERE)
        .env(ROLE, "broker")
        .env(SCRATCH, root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}

fn serve(root: &Path) {
    let lane = root.join("home/code/alpha/worktrees/lane");
    let north = root.join("north");
    write_script(&north, &format!("{ROLE}=north {SCRATCH}={} {WORDS}=\"$*\" exec {} --exact {TEST} --nocapture\n", root.display(), std::env::current_exe().unwrap().display()));
    let host = Host::own();
    let worker_path = format!("{}:{}", root.join("fakes").display(), std::env::var("PATH").unwrap());
    let worker: Vec<(String, String)> = [("PATH", worker_path.as_str()), (OWNER_ENV, "land"), (RED_FIX_ENV, "1"), (LANDER_SLOT_ENV, "1")].map(|(name, value)| (name.to_owned(), value.to_owned())).into();
    let ask = |words: &str, directory: &Path| ask(&north, &host, words, directory, &worker);
    let text = |file: &str| std::fs::read_to_string(root.join(file)).unwrap_or_default();
    let free = format!("capacity offload free --host {BUILD_HOST}");
    let mut failures = Vec::new();

    let (code, told) = ask(&free, &lane);
    let ran = text("ran");
    if code != 0 || ran.lines().collect::<Vec<_>>() != ["trusted ssh"] {
        failures.push(format!("the offload did not run the broker's own ssh and git alone, with the worker's PATH holding a fake of each: ran {ran:?}, exit {code}: {told}"));
    }

    for chosen in ["capacity offload choose", "capacity offload choose --any --taken a --avoid b --prefer c --count 2"] {
        let (code, told) = ask(chosen, &lane);
        if code != 0 || text("seen").lines().next_back().and_then(|started| started.split('|').nth(2)) != Some(chosen) {
            failures.push(format!("`{chosen}`, which reads each build host's room over ssh, was not started outside the caller's domain: exit {code}: {told}"));
        }
    }

    let asked = text("ran").lines().count();
    let (code, told) = ask(&format!("capacity offload choose --any --avoid {HERE}"), &lane);
    let claimed = text("claimed");
    if code != 1 || !told.contains("no host has room") || text("ran").lines().count() == asked || !claimed.is_empty() {
        failures.push(format!("a worker command's choose, asked when no build host has room, did not read the build host's room and leave it unclaimed: exit {code}: {told}; claims sent: {claimed:?}"));
    }

    let (code, told) = ask(&format!("capacity offload run --host {BUILD_HOST} --base HEAD --head HEAD --timeout 999999 -- true"), &lane);
    let seen = text("seen");
    let started: Vec<&str> = seen.lines().next_back().unwrap_or_default().split('|').collect();
    let [directory, claims, words] = started.as_slice() else { panic!("the run started no north: exit {code}: {told}") };
    if Path::new(directory) != lane {
        failures.push(format!("a run asked from the caller's lane started in {directory}"));
    }
    if !claims.is_empty() {
        failures.push(format!("the caller's owner and slot-class claims reached the offload: {claims}"));
    }
    if !words.contains(&format!("--timeout {LAND_CHECK_CEILING} --")) {
        failures.push(format!("a run asked for longer than the longest land check deadline ({LAND_CHECK_CEILING} s) was started as asked: {words}"));
    }

    let (code, told) = ask(&free, &root.join("elsewhere"));
    if code != 126 || !told.contains("refused") || text("seen") != seen {
        failures.push(format!("a request from a directory in no lane and no main checkout was not refused: exit {code}: {told}"));
    }
    a_request_is_served_only_from_the_checkout_north_made(root, &north, &host, &worker, &mut failures);
    git_reads_the_registered_repository_when_the_lane_s_gitfile_is_rewritten_after_the_check(root, &north, &host, &worker, &mut failures);
    a_request_from_a_directory_north_cannot_place_is_refused_for_every_verb(root, &north, &host, &worker, &mut failures);
    a_request_from_an_offload_slot_is_served_as_the_project_that_keeps_its_worktree(root, &north, &host, &worker, &mut failures);
    a_verb_that_starts_no_command_of_the_caller_s_finds_its_programs_on_the_broker_s_path(root, &north, &host, &worker, &mut failures);
    a_new_domain_s_git_grants_are_the_main_checkout_s_whatever_the_lane_s_gitfile_names(root, &mut failures);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The case of a verb that starts no command of the caller's, asked with the caller's `PATH` holding a fake ssh before the real one: the `north` started for it finds ssh on the broker's `PATH`.
fn a_verb_that_starts_no_command_of_the_caller_s_finds_its_programs_on_the_broker_s_path(root: &Path, north: &Path, host: &Host, worker: &[(String, String)], failures: &mut Vec<String>) {
    let lane = root.join("home/code/alpha/worktrees/lane");
    let ran = || std::fs::read_to_string(root.join("ran")).unwrap_or_default();
    for verb in OWN_PATH {
        let before = ran().lines().count();
        let (code, told) = ask(north, host, verb, &lane, worker);
        let found: Vec<String> = ran().lines().skip(before).map(str::to_owned).collect();
        if code != 0 || found != ["trusted ssh"] {
            failures.push(format!("a_verb_that_starts_no_command_of_the_caller_s_finds_its_programs_on_the_broker_s_path: `{verb}` ran {found:?} as ssh, with the caller's PATH holding a fake one first: exit {code}: {told}"));
        }
    }
}

/// The case of a lane whose `.git` names another repository when a domain is made for a command run from it: the trees that domain writes and removes beneath lie in the repository the project's main checkout keeps, and none in the one the lane names.
fn a_new_domain_s_git_grants_are_the_main_checkout_s_whatever_the_lane_s_gitfile_names(root: &Path, failures: &mut Vec<String>) {
    let home = root.join("home");
    let lane = home.join("code/alpha/worktrees/lane");
    let (own, other) = (home.join("code/alpha/main/.git"), root.join("elsewhere"));
    let pointer = lane.join(".git");
    let made = std::fs::read_to_string(&pointer).unwrap();
    std::fs::write(&pointer, format!("gitdir: {}\n", other.join(".git").display())).unwrap();
    let trees = removable(&Context::from_request("true", &lane, &[("HOME".to_owned(), home.display().to_string())]));
    std::fs::write(&pointer, made).unwrap();
    let named: Vec<&PathBuf> = trees.iter().filter(|tree| tree.starts_with(&other)).collect();
    if !named.is_empty() || !trees.iter().any(|tree| tree.starts_with(&own)) {
        failures.push(format!("a_new_domain_s_git_grants_are_the_main_checkout_s_whatever_the_lane_s_gitfile_names: a command run from a lane whose .git names {} may write and remove beneath {named:?}, with {} of its trees in {}", other.display(), trees.iter().filter(|tree| tree.starts_with(&own)).count(), own.display()));
    }
}

/// The case of a lane whose `.git` is rewritten to name another repository after the broker's check, before the git of the process it started reads a repository: the started process rewrites it itself and then runs git.
fn git_reads_the_registered_repository_when_the_lane_s_gitfile_is_rewritten_after_the_check(root: &Path, north: &Path, host: &Host, worker: &[(String, String)], failures: &mut Vec<String>) {
    let lane = root.join("home/code/alpha/worktrees/lane");
    let own = root.join("home/code/alpha/main/.git");
    let pointer = lane.join(".git");
    let made = std::fs::read_to_string(&pointer).unwrap();
    std::fs::write(root.join(REWRITE), format!("gitdir: {}\n", root.join("elsewhere/.git").display())).unwrap();
    let (code, told) = ask(north, host, &format!("capacity offload run --host {BUILD_HOST} --base HEAD --head HEAD --timeout 9 -- true"), &lane, worker);
    std::fs::remove_file(root.join(REWRITE)).unwrap();
    let rewritten = std::fs::read_to_string(&pointer).unwrap();
    std::fs::write(&pointer, made).unwrap();
    let read = std::fs::read_to_string(root.join("read")).unwrap_or_default();
    if code != 0 || !rewritten.contains("elsewhere") || read != format!("{}\n{}\n", own.display(), own.join("worktrees/lane").display()) {
        failures.push(format!("git_reads_the_registered_repository_when_the_lane_s_gitfile_is_rewritten_after_the_check: the offload's git read {read:?}, not the repository {} keeps for the lane, once the lane's .git said {rewritten:?}: exit {code}: {told}", own.display()));
    }
}

/// The cases of a request asked from a directory the broker cannot place in a lane or a main checkout that reads its project's own repository: one in no project, a lane of a project with no main checkout, and a lane whose `.git` leads to no repository.
fn a_request_from_a_directory_north_cannot_place_is_refused_for_every_verb(root: &Path, north: &Path, host: &Host, worker: &[(String, String)], failures: &mut Vec<String>) {
    let lane = root.join("home/code/alpha/worktrees/lane");
    let orphan = root.join("home/code/beta/worktrees/lane");
    std::fs::create_dir_all(&orphan).unwrap();
    let text = |file: &str| std::fs::read_to_string(root.join(file)).unwrap_or_default();
    let seen = text("seen");
    let offloads = [format!("capacity offload free --host {BUILD_HOST}"), format!("capacity offload run --host {BUILD_HOST} --base HEAD --head HEAD --timeout 9 -- true")];
    let verbs: Vec<&str> = offloads.iter().map(String::as_str).chain(SERVED).collect();
    let mut refused = |directory: &Path, place: &str| {
        for verb in &verbs {
            let (code, told) = ask(north, host, verb, directory, worker);
            if code != 126 || !told.contains("refused") || text("seen") != seen {
                failures.push(format!("a_request_from_a_directory_north_cannot_place_is_refused_for_every_verb: `{verb}` asked from {place} was not refused: exit {code}: {told}"));
            }
        }
    };
    refused(&root.join("elsewhere"), "a directory in no lane and no main checkout");
    refused(&orphan, "a lane of a project with no main checkout");
    let pointer = lane.join(".git");
    let made = std::fs::read_to_string(&pointer).unwrap();
    std::fs::write(&pointer, "gitdir: /nonexistent/n118\n").unwrap();
    refused(&lane, "a lane whose .git leads to no repository");
    std::fs::write(&pointer, made).unwrap();
}

/// The cases of a request asked from under the build offload's slots: a job in a slot the offload's script made as a worktree of the project's main checkout is served there, as the first runtime with the placement rule did not on 2026-11-12, and a directory made beside it is refused, with no `.git`, with the slot's own `.git` copied in, and as a repository of its own.
fn a_request_from_an_offload_slot_is_served_as_the_project_that_keeps_its_worktree(root: &Path, north: &Path, host: &Host, worker: &[(String, String)], failures: &mut Vec<String>) {
    const CASE: &str = "a_request_from_an_offload_slot_is_served_as_the_project_that_keeps_its_worktree";
    let slots = root.join("home/.cache/land-offload");
    let (slot, made_up) = (slots.join("code-alpha-main-5"), slots.join("made-up"));
    std::fs::create_dir_all(made_up.join("below")).unwrap();
    git(&root.join("home/code/alpha/main"), &["worktree", "add", "-q", "--detach", &slot.to_string_lossy()]);
    std::fs::create_dir_all(slot.join("crates")).unwrap();
    let text = |file: &str| std::fs::read_to_string(root.join(file)).unwrap_or_default();
    let verb = "depbox run -- true";
    let (code, told) = ask(north, host, verb, &slot.join("crates"), worker);
    let seen = text("seen");
    if code != 0 || seen.lines().next_back().and_then(|started| started.split('|').next()) != Some(&*slot.join("crates").to_string_lossy()) {
        failures.push(format!("{CASE}: `{verb}` asked from a job's slot was not served there: exit {code}: {told}"));
    }
    let mut refused = |place: &str| {
        let (code, told) = ask(north, host, verb, &made_up.join("below"), worker);
        if code != 126 || !told.contains("refused") || text("seen") != seen {
            failures.push(format!("{CASE}: `{verb}` asked from {place} was not refused: exit {code}: {told}"));
        }
    };
    refused("a directory made under the slots that is no checkout");
    std::fs::copy(slot.join(".git"), made_up.join(".git")).unwrap();
    refused("a directory made under the slots that holds a slot's .git");
    std::fs::remove_file(made_up.join(".git")).unwrap();
    git(&made_up, &["init", "-q", "-b", "main"]);
    refused("a repository made under the slots");
}

/// The cases of a request asked from a lane that reads another repository than its project's own, through its `.git` or its worktree's `commondir`, and of one that names another repository in its environment.
fn a_request_is_served_only_from_the_checkout_north_made(root: &Path, north: &Path, host: &Host, worker: &[(String, String)], failures: &mut Vec<String>) {
    let lane = root.join("home/code/alpha/worktrees/lane");
    let other = root.join("elsewhere/.git").display().to_string();
    let text = |file: &str| std::fs::read_to_string(root.join(file)).unwrap_or_default();
    let seen = text("seen");
    let free = format!("capacity offload free --host {BUILD_HOST}");
    let verbs: Vec<&str> = std::iter::once(free.as_str()).chain(SERVED).collect();
    let mut refused = |verbs: &[&str], through: &str| {
        for verb in verbs {
            let (code, told) = ask(north, host, verb, &lane, worker);
            if code != 126 || !told.contains(FOREIGN) || text("seen") != seen {
                failures.push(format!("`{verb}` was served from a lane whose {through} names another repository: exit {code}: {told}"));
            }
        }
    };
    let pointer = lane.join(".git");
    let made = std::fs::read_to_string(&pointer).unwrap();
    std::fs::write(&pointer, format!("gitdir: {other}\n")).unwrap();
    refused(&verbs, ".git");
    std::fs::write(&pointer, made).unwrap();
    let common = root.join("home/code/alpha/main/.git/worktrees/lane/commondir");
    let made = std::fs::read_to_string(&common).unwrap();
    std::fs::write(&common, format!("{other}\n")).unwrap();
    refused(&verbs[..1], "commondir");
    std::fs::write(&common, made).unwrap();

    let located: Vec<(String, String)> = worker.iter().cloned().chain(GIT_PLACES.map(|name| (name.to_owned(), other.clone()))).collect();
    let (code, told) = ask(north, host, "lane unlock x", &lane, &located);
    let after = text("seen");
    let places = after.strip_prefix(seen.as_str()).unwrap_or_default().split('|').nth(1).unwrap_or("GIT_ unseen: no north was started");
    if code != 0 || places.contains("GIT_") {
        failures.push(format!("a request from the lane north made, naming another repository in its environment, was not served without those names: {places}, exit {code}: {told}"));
    }
}

fn offload(root: &Path) {
    let words: Vec<String> = std::env::var(WORDS).unwrap().split(' ').map(str::to_owned).collect();
    let own = root.join("home/code/alpha");
    let elsewhere = |name: &&str| std::env::var_os(name).is_some_and(|place| !Path::new(&place).starts_with(&own));
    let claims: Vec<&str> = CLAIMS.into_iter().filter(|name| std::env::var_os(name).is_some()).chain(GIT_PLACES.into_iter().filter(elsewhere)).collect();
    let started = format!("{}|{}|{}\n", std::env::current_dir().unwrap().display(), claims.join(","), words.join(" "));
    std::fs::OpenOptions::new().append(true).create(true).open(root.join("seen")).unwrap().write_all(started.as_bytes()).unwrap();
    if OWN_PATH.contains(&words.join(" ").as_str()) {
        let _ = Command::new("ssh").output();
    }
    if let Ok(pointer) = std::fs::read_to_string(root.join(REWRITE)) {
        std::fs::write(".git", pointer).unwrap();
        let read = Command::new("git").args(["rev-parse", "--path-format=absolute", "--git-common-dir", "--absolute-git-dir"]).output().unwrap();
        std::fs::write(root.join("read"), [read.stdout, read.stderr].concat()).unwrap();
    }
    let code = if words.get(2).is_some_and(|word| word == "free") || words.last().is_some_and(|word| word == HERE) { crate::land_offload::run(&words[2..]) } else { 0 };
    std::process::exit(i32::from(code));
}

fn ask(north: &Path, host: &Host, words: &str, directory: &Path, environment: &[(String, String)]) -> (u8, String) {
    let (client, server) = UnixStream::pair().unwrap();
    let (mut out, out_writer) = std::io::pipe().unwrap();
    let (mut err, err_writer) = std::io::pipe().unwrap();
    let stdin = File::open("/dev/null").unwrap();
    let request = Request { arguments: words.split(' ').map(str::to_owned).collect(), directory: directory.to_owned(), environment: environment.to_vec() };
    let (code, logged) = std::thread::scope(|scope| {
        let served = scope.spawn(|| broker::answer(north, &server, Some(OPEN_SCOPE), host));
        let code = broker::send(&client, request, [stdin.as_fd(), out_writer.as_fd(), err_writer.as_fd()]).unwrap();
        let logged = served.join().unwrap().err().unwrap_or_default();
        (code, logged)
    });
    drop((out_writer, err_writer));
    let (mut stdout, mut stderr) = (String::new(), String::new());
    out.read_to_string(&mut stdout).unwrap();
    err.read_to_string(&mut stderr).unwrap();
    (code, format!("{stdout}{stderr}{logged}"))
}
