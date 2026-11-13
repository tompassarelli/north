//! The two records a worker session may not write itself and has the outside broker write (south#118): the build slot record an offload's script asks for, and the finish record, which the broker writes from the exit code of the acceptance command it runs for a finish call. The test starts itself again as the broker, with a home, a fleet and a `PATH` of its own, and that broker starts it once more as the `north` of the slot request, the way offload_request_tests.rs does.

use std::fs::File;
use std::io::Read;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::platform::finish_record::FinishRecords;

use super::super::broker::{self, Request};
use super::super::lane_grant::Host;
use super::{OPEN_SCOPE, git};
use crate::foundation::shim::write_script;

const ROLE: &str = "NORTH_118_RECORDS_ROLE";
const SCRATCH: &str = "NORTH_118_RECORDS_SCRATCH";
const WORDS: &str = "NORTH_118_RECORDS_WORDS";
const TEST: &str = "worker_sandbox::tests::a_worker_lane_writes_its_slot_and_finish_records_through_the_broker";
const SESSION: &str = "w-records-118";

pub(super) fn played() -> bool {
    let Some(root) = std::env::var_os(SCRATCH).map(PathBuf::from) else { return false };
    match std::env::var(ROLE).as_deref() {
        Ok("broker") => serve(&root),
        Ok("north") => {
            let words: Vec<String> = std::env::var(WORDS).unwrap().split(' ').map(str::to_owned).collect();
            if words.first().is_some_and(|word| word == super::super::FLAG) {
                let status = Command::new("sh").arg("-c").arg(words[5..].join(" ")).status().unwrap();
                std::process::exit(status.code().unwrap_or(137));
            }
            std::process::exit(i32::from(crate::land_offload::run(&words[2..])));
        }
        _ => return false,
    }
    true
}

pub(super) fn a_worker_lane_writes_its_slot_and_finish_records_through_the_broker() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().canonicalize().unwrap();
    let home = root.join("home");
    let main = home.join("code/alpha/main");
    for directory in [main.clone(), home.join("code/alpha/worktrees"), root.join("bin"), root.join("fleet").join(SESSION)] {
        std::fs::create_dir_all(directory).unwrap();
    }
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "one"]);
    git(&main, &["worktree", "add", "-q", "../worktrees/lane", "-b", "lane"]);
    git(&home.join("code/alpha/worktrees/lane"), &["commit", "-q", "--allow-empty", "-m", "two"]);
    git(&main, &["worktree", "add", "-q", "../worktrees/other", "-b", "other"]);
    git(&main, &["worktree", "add", "-q", "../worktrees/third", "-b", "third"]);
    git(&main, &["worktree", "add", "-q", "../worktrees/placed", "-b", "placed"]);
    let held = home.join(".local/state/north/held/north-fleet-w-placed-118-0a1b2c3d.service");
    std::fs::create_dir_all(&held).unwrap();
    std::fs::write(held.join("conversation"), "c-placed-118").unwrap();
    std::fs::create_dir_all(root.join("fleet/.placed/c-placed-118")).unwrap();
    std::fs::write(root.join("fleet/.placed/c-placed-118/session.json"), serde_json::json!({ "name": "w-placed-118", "cwd": home.join("code/alpha/worktrees/placed") }).to_string()).unwrap();
    std::fs::create_dir_all(root.join("fleet/w-neighbour-118")).unwrap();
    std::fs::write(root.join("fleet/w-neighbour-118/host.pid"), std::process::id().to_string()).unwrap();
    std::fs::write(root.join("fleet/w-neighbour-118/session.json"), serde_json::json!({ "name": "w-neighbour-118", "cwd": home.join("code/alpha/worktrees/third") }).to_string()).unwrap();
    write_script(&root.join("bin/ssh"), "exit 0\n");
    std::fs::create_dir_all(home.join(".local/bin")).unwrap();
    write_script(&home.join(".local/bin/gh"), &format!("# {}\n", north_base::platform::gh_shim::MARKER));
    std::fs::write(root.join("fleet").join(SESSION).join("host.scope"), "north-session-77c49ab1-cccb-4a4d-8623-d56cb8891ace.scope\n").unwrap();
    std::fs::write(root.join("fleet").join(SESSION).join("session.json"), serde_json::json!({ "name": SESSION, "cwd": main }).to_string()).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env_clear()
        .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap()))
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("NORTH_FLEET_DIR", root.join("fleet"))
        .env(ROLE, "broker")
        .env(SCRATCH, &root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}

fn serve(root: &Path) {
    let north = root.join("north");
    write_script(&north, &format!("{ROLE}=north {SCRATCH}={} {WORDS}=\"$*\" exec {} --exact {TEST} --nocapture\n", root.display(), std::env::current_exe().unwrap().display()));
    let host = Host::own();
    let lane = root.join("home/code/alpha/worktrees/lane");
    let main = root.join("home/code/alpha/main");
    let head = String::from_utf8(Command::new("git").arg("-C").arg(&lane).args(["rev-parse", "HEAD"]).output().unwrap().stdout).unwrap().trim().to_owned();
    let slots = root.join("home").join(north_base::foundation::offload::SLOT_RECORDS);
    let mut failures = Vec::new();

    let (code, told) = ask(&north, &host, OPEN_SCOPE, &["capacity", "offload", "slot", "record", "code-alpha-main-3"].map(str::to_owned), &main);
    let record = std::fs::read_to_string(slots.join("code-alpha-main-3")).unwrap_or_default();
    if code != 0 || record != "code/alpha/main\n" || !told.is_empty() {
        failures.push(format!("the slot record a job's script asks for was not written by the broker's side as the project of the checkout it was asked from: exit {code}, record {record:?}: {told}"));
    }

    let records = FinishRecords::of(&host.environment, &host.home);
    let words = |command: &str, head: &str| super::super::finish_request::request_words(command, head);
    let ran = root.join("ran");
    let marking = format!("touch {}", ran.display());
    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words("true", &head), &lane);
    let written = records.read(SESSION);
    if code != 0 || written.as_ref().is_none_or(|record| !record.passed || record.head != head || record.command != "true") || !told.is_empty() {
        failures.push(format!("the check a finish call asks for was not run by the broker, or its passing verdict was not recorded from the command's exit code for the caller's own session: exit {code}, record {written:?}: {told}"));
    }
    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words("exit 3", &head), &lane);
    if code != 3 || records.read(SESSION).is_none_or(|record| record.passed || record.command != "exit 3") || !told.is_empty() {
        failures.push(format!("a failing command was not recorded as failed over the passing record: exit {code}: {told}"));
    }
    let git_text = |directory: &Path, arguments: &[&str]| String::from_utf8(Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(arguments).output().unwrap().stdout).unwrap().trim().to_owned();
    let would_be = git_text(&lane, &["commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", "the commit a finish call would make"]);
    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words("true", &would_be), &lane);
    if code != 0 || records.read(SESSION).is_none_or(|record| !record.passed || record.head != would_be) || !told.is_empty() {
        failures.push(format!("the commit a finish call would make on the lane's tip was not accepted: exit {code}: {told}"));
    }
    let (code, _) = ask(&north, &host, OPEN_SCOPE, &words("exit 3", &head), &lane);
    if code != 3 || records.read(SESSION).is_none_or(|record| record.passed) {
        failures.push("a failing finish check did not leave a failed record to refuse against".to_owned());
    }

    let evil = {
        let blob = Command::new("sh").arg("-c").arg("printf evil | git hash-object -w --stdin").current_dir(&lane).output().unwrap();
        let blob = String::from_utf8(blob.stdout).unwrap().trim().to_owned();
        let index = root.join("evil-index");
        let with_index = |arguments: &[&str]| String::from_utf8(Command::new("git").arg("-C").arg(&lane).args(arguments).env("GIT_INDEX_FILE", &index).output().unwrap().stdout).unwrap().trim().to_owned();
        with_index(&["read-tree", "HEAD"]);
        with_index(&["update-index", "--add", "--cacheinfo", &format!("100644,{blob},evil.txt")]);
        let tree = with_index(&["write-tree"]);
        git_text(&lane, &["commit-tree", &tree, "-p", "HEAD", "-m", "a tree the check never ran on"])
    };
    let before = records.read(SESSION);
    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words("true", &evil), &lane);
    if code != 0 || before != records.read(SESSION) || !told.is_empty() {
        failures.push(format!("a verdict was recorded for a commit whose tree is not the working tree the command ran on: exit {code}: {told}"));
    }
    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words("touch stray-118; true", &head), &lane);
    let _ = std::fs::remove_file(lane.join("stray-118"));
    if code != 0 || before != records.read(SESSION) || !told.is_empty() {
        failures.push(format!("a verdict was recorded for a lane the command itself changed after the commit it names: exit {code}: {told}"));
    }

    let outside = OPEN_SCOPE.replace("north-session-77c49ab1-cccb-4a4d-8623-d56cb8891ace.scope", "north-worker-outside.service");
    let strange = OPEN_SCOPE.replace("77c49ab1", "0badc0de");
    let main_tip = git_text(&main, &["rev-parse", "HEAD"]);
    let elsewhere = root.join("elsewhere");
    let third = root.join("home/code/alpha/worktrees/third");
    let third_tip = git_text(&third, &["rev-parse", "HEAD"]);
    let placed = root.join("home/code/alpha/worktrees/placed");
    let placed_tip = git_text(&placed, &["rev-parse", "HEAD"]);
    let other = root.join("home/code/alpha/worktrees/other");
    let (code, told) = ask(&north, &host, OPEN_SCOPE, &words("true", &git_text(&other, &["rev-parse", "HEAD"])), &other);
    if code != 0 || records.read(SESSION).is_none_or(|record| !record.passed || record.head != git_text(&other, &["rev-parse", "HEAD"])) || !told.is_empty() {
        failures.push(format!("a lane of another name that the session made after it launched was not admitted: exit {code}: {told}"));
    }
    for (cgroup, request, naming, from) in [
        (outside.as_str(), words(&marking, &head), "no session's scope", &lane),
        (strange.as_str(), words(&marking, &head), "no fleet session", &lane),
        (OPEN_SCOPE, words(&marking, &main_tip), "is no commit of the lane", &lane),
        (OPEN_SCOPE, words(&marking, &"f".repeat(40)), "is no commit of the lane", &lane),
        (OPEN_SCOPE, words(&format!("{marking}; {}", "x".repeat(9000)), &head), "takes no other word", &lane),
        (OPEN_SCOPE, [words(&marking, &head), vec!["passed".to_owned()]].concat(), "takes no other word", &lane),
        (OPEN_SCOPE, words(&marking, &head), "in no lane", &elsewhere),
        (OPEN_SCOPE, words(&marking, &main_tip), "is no lane of", &main),
        (OPEN_SCOPE, words(&marking, &third_tip), "launched in, not one of", &third),
        (OPEN_SCOPE, words(&marking, &placed_tip), "launched in, not one of", &placed),
    ] {
        let before = records.read(SESSION);
        let (code, told) = ask(&north, &host, cgroup, &request, from);
        if code != 126 || !told.contains(naming) || before != records.read(SESSION) || ran.exists() {
            failures.push(format!("a finish check asked for with {naming:?} against it was not refused, or ran, or changed the record: exit {code}: {told}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn ask(north: &Path, host: &Host, cgroup: &str, arguments: &[String], directory: &Path) -> (u8, String) {
    let (client, server) = UnixStream::pair().unwrap();
    let (mut out, out_writer) = std::io::pipe().unwrap();
    let (mut err, err_writer) = std::io::pipe().unwrap();
    let stdin = File::open("/dev/null").unwrap();
    let request = Request { arguments: arguments.to_vec(), directory: directory.to_owned(), environment: Vec::new() };
    let code = std::thread::scope(|scope| {
        scope.spawn(|| broker::answer(north, &server, Some(cgroup), host));
        broker::send(&client, request, [stdin.as_fd(), out_writer.as_fd(), err_writer.as_fd()]).unwrap()
    });
    drop((out_writer, err_writer));
    let (mut stdout, mut stderr) = (String::new(), String::new());
    out.read_to_string(&mut stdout).unwrap();
    err.read_to_string(&mut stderr).unwrap();
    drop(stdout);
    (code, stderr)
}
