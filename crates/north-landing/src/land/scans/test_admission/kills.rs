use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use toml_edit::DocumentMut;

use crate::platform::push_ledger;
use super::super::{Range, git};
use super::limits;
use crate::watch::state_home;

pub const USAGE: &str = "usage: north land scan test-admission kills [--host NAME] [--remote-north COMMAND] BASE..HEAD\n       north land scan test-admission kills --here BASE..HEAD";
const UNAVAILABLE: u8 = 75;
const SHIP_SECONDS: u64 = 900;
const WORK: &str = "target/mutant-kills";
const POLL: Duration = Duration::from_secs(2);
const GRACE: Duration = Duration::from_secs(30);
const SHOWN: usize = 5;
const BASELINE_FAILED: i32 = 4;

struct Lane {
    base: String,
    head: String,
    names: BTreeSet<String>,
    files: Vec<String>,
    sources: Vec<String>,
}

enum End {
    Exited(ExitStatus),
    Caught,
    Capped,
}

pub fn run(directory: &Path, arguments: &[String], out: &mut impl Write) -> u8 {
    let (mut host, mut remote, mut here, mut range, mut bad) = (None, "north".to_owned(), false, None, false);
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--here" => here = true,
            "--host" => match words.next() {
                Some(name) => host = Some(name.clone()),
                None => bad = true,
            },
            "--remote-north" => remote = words.next().cloned().unwrap_or_default(),
            value if range.is_none() && !value.starts_with('-') => range = Some(value.to_owned()),
            _ => bad = true,
        }
    }
    let Some(lane) = range.as_deref().filter(|_| !bad && !remote.is_empty()).and_then(|range| lane(directory, range)) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let Some(lane) = lane else {
        let _ = writeln!(out, "mutant kill: not applicable (the lane adds no Rust test next to a change in its mutated source)");
        return 0;
    };
    if here { on_this_host(directory, &lane, out) } else { offloaded(directory, &lane, host, &remote, out) }
}

fn lane(directory: &Path, range: &str) -> Option<Option<Lane>> {
    let Range { base, head } = Range::parse(range)?;
    let base = git::merge_base(directory, base, head);
    let head = git::text(directory, &["rev-parse", "--verify", &format!("{head}^{{commit}}")])?.trim().to_owned();
    let found = super::changes(directory, &base, &head);
    let names = super::fresh_names(directory, &base, &found.patch);
    let sources: Vec<String> = git::text(directory, &["config", "--blob", &format!("{base}:.safe-push"), "--get-all", "mutant.source"]).unwrap_or_default().lines().map(str::to_owned).collect();
    let unchanged = git::bytes(directory, &[&["diff", "--quiet", &base, &head, "--"][..], &sources.iter().map(String::as_str).collect::<Vec<_>>()].concat()).is_some();
    let files = found.files.into_iter().filter(|file| file.ends_with(".rs")).collect();
    Some((!names.is_empty() && !unchanged).then_some(Lane { base, head, names, files, sources }))
}

fn input_key(directory: &Path, lane: &Lane, cap: u64) -> Option<String> {
    let tree = |commit: &str| git::text(directory, &["rev-parse", &format!("{commit}^{{tree}}")]).map(|tree| tree.trim().to_owned());
    let text = format!("mutant-kill v1\0{}\0{}\0{cap}", tree(&lane.base)?, tree(&lane.head)?);
    Some(Sha256::digest(text).iter().map(|byte| format!("{byte:02x}")).collect())
}

fn offloaded(directory: &Path, lane: &Lane, host: Option<String>, remote: &str, out: &mut impl Write) -> u8 {
    let cap = limits::kill_seconds();
    let origin = git::text(directory, &["remote", "get-url", "origin"]).unwrap_or_default();
    let store = state_home().join("agents/mutants").join(push_ledger::repository(origin.trim())).join("kills");
    let key = input_key(directory, lane, cap);
    if let Some(stored) = key.as_ref().and_then(|key| fs::read_to_string(store.join(key)).ok()) {
        let (code, verdict) = stored.split_once('\n').unwrap_or((&stored, ""));
        if let Ok(code) = code.parse::<u8>() {
            let _ = writeln!(out, "mutant kill: reusing the verdict for unchanged input {}", &key.as_deref().unwrap_or_default()[..12]);
            let _ = out.write_all(verdict.as_bytes());
            return code;
        }
    }
    let Some(host) = host.or_else(|| choose(directory)) else {
        let _ = writeln!(out, "mutant kill: could not run, not the lane's fault: no build host has room (north capacity offload choose --any) and .safe-push declares no landing.remote-hosts");
        return UNAVAILABLE;
    };
    let command = format!("{remote} land scan test-admission kills --here {}..{}", lane.base, lane.head);
    let _ = writeln!(out, "mutant kill: running cargo-mutants over the lane's diff on {host}, capped at {cap} s, for {}", lane.names.iter().cloned().collect::<Vec<_>>().join(", "));
    let ran = Command::new(north_base::platform::north_binary::north_binary())
        .current_dir(directory)
        .args(["capacity", "offload", "run", "--host", &host, "--base", &lane.base, "--head", &lane.head, "--timeout", &(cap + SHIP_SECONDS).to_string(), "--", &command])
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output();
    let Ok(output) = ran else {
        let _ = writeln!(out, "mutant kill: could not run, not the lane's fault: north capacity offload run did not start");
        return UNAVAILABLE;
    };
    let _ = out.write_all(&output.stdout);
    let verdict: String = String::from_utf8_lossy(&output.stdout).lines().filter(|line| line.starts_with("mutant kill:") || line.starts_with("safe-push: ✗ mutant kill")).map(|line| format!("{line}\n")).collect();
    let code = match output.status.code() {
        Some(0) if !verdict.is_empty() => 0,
        Some(1) if verdict.contains("safe-push: ✗ mutant kill") => 1,
        _ => unavailable(out, &format!("north capacity offload run on {host} exited {}, so no verdict was reached", output.status)),
    };
    if let Some(key) = key.filter(|_| code <= 1 && !verdict.is_empty()) {
        let _ = fs::create_dir_all(&store).and_then(|()| fs::write(store.join(key), format!("{code}\n{verdict}")));
    }
    code
}

fn choose(directory: &Path) -> Option<String> {
    let here = north_base::platform::host::name();
    let output = Command::new(north_base::platform::north_binary::north_binary()).current_dir(directory).args(["capacity", "offload", "choose", "--any", "--avoid", &here]).stdin(Stdio::null()).stderr(Stdio::inherit()).output().ok()?;
    let host = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let declared = || git::text(directory, &["config", "--file", ".safe-push", "--get", "landing.remote-hosts"]).and_then(|hosts| hosts.split_whitespace().next().map(str::to_owned));
    (output.status.success() && !host.is_empty()).then_some(host).or_else(declared)
}

fn on_this_host(directory: &Path, lane: &Lane, out: &mut impl Write) -> u8 {
    let Some(top) = git::text(directory, &["rev-parse", "--show-toplevel"]).map(|top| PathBuf::from(top.trim())) else {
        return unavailable(out, "not inside a git checkout");
    };
    if git::text(&top, &["rev-parse", "HEAD"]).is_none_or(|head| head.trim() != lane.head) || git::text(&top, &["status", "--porcelain", "--untracked-files=no"]).is_none_or(|status| !status.is_empty()) {
        return unavailable(out, &format!("--here mutates the checkout in place, so it needs a clean checkout of {}", lane.head));
    }
    let work = top.join(WORK);
    let Some(diff) = git::bytes(&top, &[&["diff", "--no-color", "--no-ext-diff", &lane.base, &lane.head, "--"][..], &lane.sources.iter().map(String::as_str).collect::<Vec<_>>()].concat()) else {
        return unavailable(out, &format!("git diff {}..{} failed", lane.base, lane.head));
    };
    let _ = fs::remove_dir_all(&work);
    if let Err(error) = fs::create_dir_all(&work).and_then(|()| fs::write(work.join("lane.diff"), diff)) {
        return unavailable(out, &format!("{}: {error}", work.display()));
    }
    let in_diff = ["--in-diff".to_owned(), work.join("lane.diff").display().to_string()];
    let listed = Command::new("cargo").current_dir(&top).args(["mutants", "--workspace", "--list"]).args(&in_diff).stdin(Stdio::null()).stderr(Stdio::inherit()).output();
    let total = match listed {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).lines().filter(|line| !line.trim().is_empty()).count(),
        _ => return unavailable(out, "cargo mutants --list failed; run it inside the repository's dev shell"),
    };
    if total == 0 {
        let _ = writeln!(out, "mutant kill: not applicable (cargo-mutants finds no mutant in the lane's diff)");
        return 0;
    }
    let cap = limits::kill_seconds();
    let started = Instant::now();
    let end = match beside_tests(&top, lane, &work) {
        Some(near) => match mutate(&top, &work, lane, &near, cap) {
            Ok(End::Exited(status)) if status.code() != Some(BASELINE_FAILED) => {
                restore(&top);
                mutate(&top, &work, lane, &in_diff, cap.saturating_sub(started.elapsed().as_secs()))
            }
            first => first,
        },
        None => mutate(&top, &work, lane, &in_diff, cap),
    };
    restore(&top);
    match end {
        Ok(end) => judge(&work, lane, &end, Tally { total, cap, seconds: started.elapsed().as_secs() }, out),
        Err(error) => unavailable(out, &format!("cargo mutants: {error}")),
    }
}

struct Tally {
    total: usize,
    cap: u64,
    seconds: u64,
}

fn judge(work: &Path, lane: &Lane, end: &End, tally: Tally, out: &mut impl Write) -> u8 {
    let Tally { total, cap, seconds } = tally;
    let listed = |kind: &str| -> Vec<String> { fs::read_to_string(work.join("mutants.out").join(format!("{kind}.txt"))).unwrap_or_default().lines().filter(|line| !line.trim().is_empty()).map(str::to_owned).collect() };
    let (mut caught, survived) = (listed("caught"), listed("missed"));
    caught.extend(listed("timeout").into_iter().map(|mutant| format!("{mutant} (the tests time out)")));
    let tested = caught.len() + survived.len() + listed("unviable").len();
    let names = lane.names.iter().cloned().collect::<Vec<_>>().join(", ");
    if let Some(first) = caught.first() {
        let _ = writeln!(out, "mutant kill: ✓ {names} kill {first} ({tested} of {total} mutant(s) of the lane's diff tested, {seconds} s)");
        return 0;
    }
    let shown = survived.iter().take(SHOWN).cloned().collect::<Vec<_>>().join("; ");
    match end {
        End::Exited(status) if status.code() == Some(BASELINE_FAILED) => {
            unavailable(out, &format!("{names} fail on the lane's own unmutated tree, so no mutant can be judged; runner output ends:\n{}", tail(work)))
        }
        End::Exited(status) if status.code().is_none_or(|code| ![0, 2, 3].contains(&code)) => unavailable(out, &format!("cargo mutants exited {status}; its output ends:\n{}", tail(work))),
        End::Capped if tested < total => unavailable(out, &format!("the {cap} s cap passed after {tested} of {total} mutant(s), none killed yet, so the lane is not judged")),
        _ if survived.is_empty() => {
            let _ = writeln!(out, "mutant kill: not applicable (none of the {total} mutant(s) of the lane's diff builds)");
            0
        }
        _ => {
            let _ = writeln!(out, "safe-push: ✗ mutant kill: {names} survive all {} mutant(s) of the lane's diff ({tested} tested), so the added tests guard none of the change: {shown}; make a test fail when the changed code is broken; NOT pushed.", survived.len());
            1
        }
    }
}

fn mutate(top: &Path, work: &Path, lane: &Lane, in_diff: &[String], cap: u64) -> std::io::Result<End> {
    let log = fs::File::create(work.join("runner.log"))?;
    let mut command = Command::new("cargo");
    command.current_dir(top).args(["mutants", "--workspace", "--in-place", "--no-shuffle", "--output"]).arg(work).args(in_diff);
    for package in packages(top, &lane.files) {
        command.args(["--test-package", &package]);
    }
    command.args(["--", "--tests", "--"]).args(&lane.names).stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log);
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(cap);
    let kills = [work.join("mutants.out/caught.txt"), work.join("mutants.out/timeout.txt")];
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(End::Exited(status));
        }
        if kills.iter().any(|file| fs::metadata(file).is_ok_and(|meta| meta.len() > 0)) {
            stop(&mut child);
            return Ok(End::Caught);
        }
        if Instant::now() >= deadline {
            stop(&mut child);
            return Ok(End::Capped);
        }
        std::thread::sleep(POLL);
    }
}

fn stop(child: &mut Child) {
    if let Some(pid) = rustix::process::Pid::from_raw(child.id().cast_signed()) {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::Int);
    }
    let deadline = Instant::now() + GRACE;
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(POLL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn beside_tests(top: &Path, lane: &Lane, work: &Path) -> Option<[String; 2]> {
    let near: BTreeSet<String> = lane.files.iter().flat_map(|file| {
        let path = Path::new(file);
        let parent = path.parent().unwrap_or(Path::new(""));
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        match name.strip_suffix("_tests.rs") {
            Some(stem) => vec![parent.join(format!("{stem}.rs"))],
            None if name == "tests.rs" => vec![parent.join("mod.rs"), parent.with_extension("rs")],
            None => vec![path.to_path_buf()],
        }
    }).map(|path| path.display().to_string()).collect();
    let sources: Vec<&str> = lane.sources.iter().map(String::as_str).collect();
    let changed = git::text(top, &[&["diff", "--name-only", "--no-renames", &lane.base, &lane.head, "--"][..], &sources].concat())?;
    let near: Vec<&str> = changed.lines().filter(|path| near.contains(*path)).collect();
    let diff = git::bytes(top, &[&["diff", "--no-color", "--no-ext-diff", &lane.base, &lane.head, "--"][..], &near].concat()).filter(|diff| !near.is_empty() && !diff.is_empty())?;
    fs::write(work.join("near.diff"), diff).ok()?;
    Some(["--in-diff".to_owned(), work.join("near.diff").display().to_string()])
}

fn restore(top: &Path) {
    let _ = git::bytes(top, &["checkout", "-q", "--", "."]);
}

fn packages(top: &Path, files: &[String]) -> BTreeSet<String> {
    files
        .iter()
        .filter_map(|file| {
            top.join(file).ancestors().skip(1).take_while(|dir| dir.starts_with(top)).find_map(|dir| {
                let manifest = fs::read_to_string(dir.join("Cargo.toml")).ok()?.parse::<DocumentMut>().ok()?;
                manifest.get("package")?.get("name")?.as_str().map(str::to_owned)
            })
        })
        .collect()
}

fn tail(work: &Path) -> String {
    let text = fs::read_to_string(work.join("runner.log")).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(20)..].join("\n")
}

fn unavailable(out: &mut impl Write, reason: &str) -> u8 {
    let _ = writeln!(out, "mutant kill: could not run, not the lane's fault: {reason}");
    UNAVAILABLE
}
