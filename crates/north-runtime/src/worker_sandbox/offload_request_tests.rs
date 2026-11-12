//! The cases of the process the outside broker starts for a worker command's offload request (south#118). The test that runs them starts itself again as a broker with a home, a `PATH` and a host declaration of its own, and that broker starts it once more as the `north` of each request, which records what it was started with and, for `free` and the `choose` that leaves this host out, runs the offload verb itself.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::foundation::offload::{LANDER_SLOT_ENV, OWNER_ENV, RED_FIX_ENV};

use super::super::broker::{self, Request};
use super::super::broker_served::Served;
use super::super::broker_start;
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
    a_request_from_an_offload_slot_is_served_only_as_the_project_the_offload_recorded_for_it(root, &north, &host, &worker, &mut failures);
    a_canary_probe_s_scratch_project_is_served_by_the_broker_the_probe_starts_for_it(root, &north, &worker, &mut failures);
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

/// The cases of a request asked from under the build offload's slots: a job in a slot the offload's script made and recorded as its project's is served there, as the first runtime with the placement rule did not on 2026-11-12, and nothing else is: the same slot before its record, a directory beside it that holds the slot's `.git`, and a directory named as another session's lane whose `.git` names that lane's worktree, with a record of that name or without, which a rule that read the directory's own `.git` served as that lane's repository.
fn a_request_from_an_offload_slot_is_served_only_as_the_project_the_offload_recorded_for_it(root: &Path, north: &Path, host: &Host, worker: &[(String, String)], failures: &mut Vec<String>) {
    const CASE: &str = "a_request_from_an_offload_slot_is_served_only_as_the_project_the_offload_recorded_for_it";
    let home = root.join("home");
    let (slots, records) = (home.join(north_base::foundation::offload::ROOT_DEFAULT), home.join(north_base::foundation::offload::SLOT_RECORDS));
    let (slot, made_up, named_as_lane) = (slots.join("code-alpha-main-5"), slots.join("made-up"), slots.join("lane"));
    for directory in [&made_up, &named_as_lane, &records] {
        std::fs::create_dir_all(directory).unwrap();
    }
    git(&home.join("code/alpha/main"), &["worktree", "add", "-q", "--detach", &slot.to_string_lossy()]);
    std::fs::copy(slot.join(".git"), made_up.join(".git")).unwrap();
    std::fs::copy(home.join("code/alpha/worktrees/lane/.git"), named_as_lane.join(".git")).unwrap();
    let text = |file: &str| std::fs::read_to_string(root.join(file)).unwrap_or_default();
    let verb = "depbox run -- true";
    let mut refused = |directory: &Path, place: &str| {
        let seen = text("seen");
        let (code, told) = ask(north, host, verb, directory, worker);
        if code != 126 || !told.contains("refused") || text("seen") != seen {
            failures.push(format!("{CASE}: `{verb}` asked from {place} was not refused: exit {code}: {told}"));
        }
    };
    refused(&slot, "a slot the offload recorded no project for");
    refused(&made_up, "a directory made under the slots that holds a slot's .git");
    refused(&named_as_lane, "a directory made under the slots, named as a lane, whose .git names that lane's worktree");
    std::fs::write(records.join("lane"), "code/alpha/main
").unwrap();
    refused(&named_as_lane, "a recorded slot whose name is a lane of its project");
    let lane = home.join("code/alpha/worktrees/lane");
    let recorded = |name: &str| (ask(north, host, &format!("capacity offload slot record {name}"), &lane, worker), std::fs::read_to_string(records.join(name)).unwrap_or_default());
    let mut unrecorded = Vec::new();
    let ((code, told), record) = recorded("made-up");
    if code == 0 || !record.is_empty() {
        unrecorded.push(format!("{CASE}: a session had a name that is no slot of its project recorded as one: exit {code}, record {record:?}: {told}"));
    }
    let ((code, told), record) = recorded("code-alpha-main-5");
    if code != 0 || record != "code/alpha/main\n" {
        unrecorded.push(format!("{CASE}: the record a job's script asks for from inside a session's domain, where it cannot write it, was not written by the broker's side as the project of the checkout the request was placed in: exit {code}, record {record:?}: {told}"));
    }
    let (code, told) = ask(north, host, verb, &slot, worker);
    let served = code == 0 && text("seen").lines().next_back().and_then(|started| started.split('|').next()) == Some(&*slot.to_string_lossy());
    std::fs::remove_file(records.join("code-alpha-main-5")).unwrap();
    std::fs::remove_dir_all(&slot).unwrap();
    std::fs::create_dir_all(&slot).unwrap();
    std::fs::copy(made_up.join(".git"), slot.join(".git")).unwrap();
    refused(&slot, "a directory made again under the name of a slot whose job ended, its record gone with the job while git still keeps the slot's worktree entry");
    failures.extend(unrecorded);
    if !served {
        failures.push(format!("{CASE}: `{verb}` asked from a job's recorded slot was not served there: exit {code}: {told}"));
    }
}

/// The case of the two canary probes that ask for a verb from a scratch project they make under a temporary directory (runtime/canary/probes.rs), run as an order issued from a session's window runs them, inside a domain: each starts a broker of the judged north for its scratch root on a socket there and asks on that socket, and a broker that serves that root serves the request in the probe's directory. The host's own broker refuses it, as it refused both probes of the order of 2026-11-12, which asked it.
fn a_canary_probe_s_scratch_project_is_served_by_the_broker_the_probe_starts_for_it(root: &Path, north: &Path, worker: &[(String, String)], failures: &mut Vec<String>) {
    const CASE: &str = "a_canary_probe_s_scratch_project_is_served_by_the_broker_the_probe_starts_for_it";
    let (stub, temporary, judged) = (root.join("canary"), root.join("canary-tmp"), root.join("judged-north"));
    for directory in [&stub, &temporary] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let record = "printf '%s\\n' \"$*\" > \"$stub/words\"; pwd -P > \"$stub/directory\"; : > \"$stub/asked\"";
    let wait = "waited=0; while [ ! -e \"$stub/answer\" ] && [ \"$waited\" -lt 600 ]; do sleep 0.1; waited=$((waited + 1)); done; exit \"$(cat \"$stub/answer\" 2>/dev/null || echo 99)\"";
    write_script(
        &judged,
        &format!(
            "stub={}\ncase \"$*\" in\n\"{}\") printf '%s\\n' \"$4\" > \"$stub/code\"; printf '%s\\n' \"${{{}:-}}\" > \"$stub/socket\" ;;\n\"depbox run -- true\" | \"safe-push --to main\") {record}; {wait} ;;\nesac\n",
            stub.display(),
            crate::worker_sandbox::broker_for("\"*\"").join(" "),
            broker::SOCKET_ENV
        ),
    );
    for (script, verb) in crate::runtime::scratch_project_probes(&judged.to_string_lossy()).into_iter().zip(["depbox run -- true", "safe-push --to main"]) {
        for file in ["code", "socket", "words", "directory", "asked", "answer"] {
            let _ = std::fs::remove_file(stub.join(file));
        }
        let mut probe = Command::new("bash").arg("-c").arg(&script).env(super::super::MARK, "1").env("TMPDIR", &temporary).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let asked = north_base::platform::wait::until(std::time::Duration::from_secs(60), || stub.join("asked").exists() || !matches!(probe.try_wait(), Ok(None))) && stub.join("asked").exists();
        let read = |file: &str| std::fs::read_to_string(stub.join(file)).unwrap_or_default().trim().to_owned();
        let (words, directory, code, socket) = (read("words"), PathBuf::from(read("directory")), read("code"), read("socket"));
        let started = !code.is_empty() && Path::new(&socket).parent() == Some(Path::new(&code));
        let own = Host::own();
        let (code_of_host, told_by_host) = ask(north, &own, verb, &directory, worker);
        let (exit, told, served, elsewhere) = answers_of_a_probe_s_broker(root, north, started.then(|| PathBuf::from(&code)), verb, &directory, worker);
        if let Some(answered) = elsewhere {
            failures.push(format!("{CASE}: the broker the probe started for its scratch project, where every session of the host writes, answered a `{verb}` asked from another session's cgroup: {answered}"));
        }
        std::fs::write(stub.join("answer"), "0").unwrap();
        let _ = probe.wait();
        if !asked || words != verb {
            failures.push(format!("{CASE}: the probe asked for no `{verb}`: it asked {words:?} from {}", directory.display()));
        } else if exit != 0 || !served {
            failures.push(format!("{CASE}: the probe's `{verb}`, asked from {} on the socket {socket:?} of {}, was not served there: exit {exit}: {told}", directory.display(), if started { format!("the broker it started for {code}") } else { "no broker of its own, so of the host's".to_owned() }));
        }
        if code_of_host != 126 || !told_by_host.contains("refused") {
            failures.push(format!("{CASE}: the host's own broker served the probe's `{verb}` from the scratch project {}: exit {code_of_host}: {told_by_host}", directory.display()));
        }
    }
}

/// The answers of the broker a probe starts for the projects under `code` (for none, when it started none) to `verb` asked from `directory`: to a caller in the probe's own cgroup, its exit code, what it told and whether the verb started in `directory`; and how it answered a caller in another session's cgroup, when it did not refuse that one and start nothing.
fn answers_of_a_probe_s_broker(root: &Path, north: &Path, code: Option<PathBuf>, verb: &str, directory: &Path, worker: &[(String, String)]) -> (u8, String, bool, Option<String>) {
    let started = || std::fs::read_to_string(root.join("seen")).unwrap_or_default();
    let serving = Served { code, peers: Some(OPEN_SCOPE.trim().to_owned()) };
    let from = |cgroup: &str| asked(|server| broker_start::answer(north, server, Some(cgroup), &serving), verb, directory, worker);
    let seen = started();
    let (exit, told) = from(OPEN_SCOPE);
    let served = started().strip_prefix(&seen).and_then(|new| new.lines().next_back()).and_then(|line| line.split('|').next()) == Some(&*directory.to_string_lossy());
    let seen = started();
    let (code, told_elsewhere) = from(&OPEN_SCOPE.replace("77c49ab1", "0badc0de"));
    let elsewhere = (code != 126 || !told_elsewhere.contains("refused") || started() != seen).then(|| format!("exit {code}: {told_elsewhere}"));
    (exit, told, served, elsewhere)
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
    let code = if words.get(2).is_some_and(|word| word == "free" || word == "slot") || words.last().is_some_and(|word| word == HERE) { crate::land_offload::run(&words[2..]) } else { 0 };
    std::process::exit(i32::from(code));
}

fn ask(north: &Path, host: &Host, words: &str, directory: &Path, environment: &[(String, String)]) -> (u8, String) {
    asked(|server| broker::answer(north, server, Some(OPEN_SCOPE), host), words, directory, environment)
}

/// What a caller that asks for `words` from `directory` gets from the broker `answer` plays on the other end of its socket: its exit code, and what it was told with what the broker logged.
fn asked(answer: impl FnOnce(&UnixStream) -> Result<(), String> + Send, words: &str, directory: &Path, environment: &[(String, String)]) -> (u8, String) {
    let (client, server) = UnixStream::pair().unwrap();
    let (mut out, out_writer) = std::io::pipe().unwrap();
    let (mut err, err_writer) = std::io::pipe().unwrap();
    let stdin = File::open("/dev/null").unwrap();
    let request = Request { arguments: words.split(' ').map(str::to_owned).collect(), directory: directory.to_owned(), environment: environment.to_vec() };
    let (code, logged) = std::thread::scope(|scope| {
        let served = scope.spawn(|| answer(&server));
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
