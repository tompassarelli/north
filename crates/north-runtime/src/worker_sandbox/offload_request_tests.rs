//! The cases of the process the outside broker starts for a worker command's offload request (south#118). The test that runs them starts itself again as a broker with a home, a `PATH` and a host declaration of its own, and that broker starts it once more as the `north` of each request, which records what it was started with and, for `free`, runs the offload verb itself.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::foundation::offload::{LANDER_SLOT_ENV, OWNER_ENV, RED_FIX_ENV};

use super::super::broker::{self, Request};
use super::super::lane_grant::Host;
use super::{OPEN_SCOPE, git};
use crate::foundation::shim::write_script;

const ROLE: &str = "NORTH_118_OFFLOAD_ROLE";
const SCRATCH: &str = "NORTH_118_OFFLOAD_SCRATCH";
const WORDS: &str = "NORTH_118_OFFLOAD_WORDS";
const TEST: &str = "worker_sandbox::tests::the_outside_broker_runs_only_north_s_fixed_verbs_with_an_environment_that_carries_no_injected_code";
const BUILD_HOST: &str = "n118-build";
const CLAIMS: [&str; 3] = [OWNER_ENV, RED_FIX_ENV, LANDER_SLOT_ENV];
const LAND_CHECK_CEILING: &str = "2100";
const GIT_PLACES: [&str; 3] = ["GIT_DIR", "GIT_COMMON_DIR", "GIT_WORK_TREE"];
const FOREIGN: &str = "names another repository";
const SERVED: [&str; 15] = [
    "safe-push --to main",
    "task check x",
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
    std::fs::write(policy.join("orchestration.toml"), format!("[hosts.{BUILD_HOST}]\nssh = \"{BUILD_HOST}.invalid\"\n")).unwrap();
    let path = std::env::var("PATH").unwrap();
    let real_git = std::env::split_paths(&path).map(|directory| directory.join("git")).find(|git| git.is_file()).unwrap();
    let ran = root.join("ran");
    write_script(&root.join("trusted/ssh"), &format!("echo 'trusted ssh' >> {}\necho 3\n", ran.display()));
    write_script(&root.join("fakes/ssh"), &format!("echo 'fake ssh' >> {}\necho 3\n", ran.display()));
    write_script(&root.join("fakes/git"), &format!("echo 'fake git' >> {}\nexec {} \"$@\"\n", ran.display(), real_git.display()));
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env_clear()
        .env("PATH", format!("{}:{path}", root.join("trusted").display()))
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
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
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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
    let claims: Vec<&str> = CLAIMS.into_iter().chain(GIT_PLACES).filter(|name| std::env::var_os(name).is_some()).collect();
    let started = format!("{}|{}|{}\n", std::env::current_dir().unwrap().display(), claims.join(","), words.join(" "));
    std::fs::OpenOptions::new().append(true).create(true).open(root.join("seen")).unwrap().write_all(started.as_bytes()).unwrap();
    let code = if words.get(2).is_some_and(|word| word == "free") { crate::land_offload::run(&words[2..]) } else { 0 };
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
