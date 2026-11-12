use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use toml_edit::DocumentMut;

use north_build_system::{MutantStep, Mutants, Shell};
use super::own_tree::OwnTree;
use super::{BASELINE_FAILED, End, Lane, SHOWN, Tally, git, judge, sweep_cap, unavailable};

const WORK: &str = "target/mutant-kills";
const POLL: Duration = Duration::from_secs(2);
const GRACE: Duration = Duration::from_secs(30);

pub(super) fn checkout_of(directory: &Path, head: &str) -> Option<PathBuf> {
    let top = git::text(directory, &["rev-parse", "--show-toplevel"]).map(|top| PathBuf::from(top.trim()))?;
    let at_head = git::text(&top, &["rev-parse", "HEAD"]).is_some_and(|at| at.trim() == head);
    let clean = git::text(&top, &["status", "--porcelain", "--untracked-files=no"]).is_some_and(|status| status.is_empty());
    (at_head && clean).then_some(top)
}

pub(super) fn on_this_host(directory: &Path, lane: &Lane, out: &mut impl Write) -> u8 {
    let Some(top) = checkout_of(directory, &lane.head) else {
        return unavailable(out, &format!("--here copies the checkout its land check built, so it needs a clean checkout of {}", lane.head));
    };
    let cap = match sweep_cap() {
        Ok(cap) => cap,
        Err(why) => return unavailable(out, &why),
    };
    let copying = Instant::now();
    let tree = match OwnTree::of(&top, &lane.head) {
        Ok(tree) => tree,
        Err(why) => return unavailable(out, &format!("no copy of the check's checkout and build: {why}")),
    };
    let mut judged = Vec::new();
    let code = sweep(tree.root(), lane, cap, copying.elapsed().as_secs(), &mut judged);
    let written = tree.written_through();
    if !written.is_empty() {
        return unavailable(out, &format!("the sweep wrote through a hard link into {} file(s) of the check's build directory, removed there so cargo rebuilds them: {}", written.len(), written.iter().take(SHOWN).map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ")));
    }
    let _ = out.write_all(&judged);
    code
}

fn sweep(own: &Path, lane: &Lane, cap: u64, copied: u64, out: &mut impl Write) -> u8 {
    let work = own.join(WORK);
    let Some(diff) = git::bytes(own, &[&["diff", "--no-color", "--no-ext-diff", &lane.base, &lane.head, "--"][..], &lane.sources.iter().map(String::as_str).collect::<Vec<_>>()].concat()) else {
        return unavailable(out, &format!("git diff {}..{} failed", lane.base, lane.head));
    };
    if let Err(error) = fs::create_dir_all(&work).and_then(|()| fs::write(work.join("lane.diff"), diff)) {
        return unavailable(out, &format!("{}: {error}", work.display()));
    }
    let in_diff = work.join("lane.diff");
    let lister = match north_build_system::here().mutants(&Mutants { root: own, diff: &in_diff, step: MutantStep::List, shell: Shell::LandShell }) {
        Ok(lister) => lister,
        Err(why) => return unavailable(out, &why),
    };
    let total = match lister.command().stdin(Stdio::null()).stderr(Stdio::inherit()).output() {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).lines().filter(|line| !line.trim().is_empty()).count(),
        _ => return unavailable(out, "cargo mutants --list failed in the repository's dev shell"),
    };
    if total == 0 {
        let _ = writeln!(out, "mutant kill: not applicable (cargo-mutants finds no mutant in the lane's diff)");
        return 0;
    }
    let started = Instant::now();
    let end = match beside_tests(own, lane, &work) {
        Some(near) => match mutate(own, &work, lane, &near, cap) {
            Ok(End::Exited(status)) if status.code() != Some(BASELINE_FAILED) => mutate(own, &work, lane, &in_diff, cap.saturating_sub(started.elapsed().as_secs())),
            first => first,
        },
        None => mutate(own, &work, lane, &in_diff, cap),
    };
    match end {
        Ok(end) => {
            let baseline = baseline_seconds(&work).map_or_else(|| "an unrecorded time".to_owned(), |seconds| format!("{seconds:.1} s"));
            let _ = writeln!(out, "mutant kill: copied the check's checkout and build in {copied} s; the unmutated baseline built and ran in {baseline}");
            judge(&work, lane, &end, Tally { total, cap, seconds: started.elapsed().as_secs() }, out)
        }
        Err(error) => unavailable(out, &format!("cargo mutants: {error}")),
    }
}

fn baseline_seconds(work: &Path) -> Option<f64> {
    let outcomes: serde_json::Value = serde_json::from_str(&fs::read_to_string(work.join("mutants.out/outcomes.json")).ok()?).ok()?;
    let baseline = outcomes["outcomes"].as_array()?.iter().find(|outcome| outcome["scenario"] == "Baseline")?;
    Some(baseline["phase_results"].as_array()?.iter().filter_map(|phase| phase["duration"].as_f64()).sum())
}

fn mutate(top: &Path, work: &Path, lane: &Lane, in_diff: &Path, cap: u64) -> std::io::Result<End> {
    let log = fs::File::create(work.join("runner.log"))?;
    let (packages, tests) = (packages(top, &lane.files).into_iter().collect::<Vec<_>>(), lane.names.iter().cloned().collect::<Vec<_>>());
    let step = MutantStep::Run { output: work, packages: &packages, tests: &tests };
    let runner = north_build_system::here().mutants(&Mutants { root: top, diff: in_diff, step, shell: Shell::LandShell }).map_err(std::io::Error::other)?;
    let mut command = runner.command();
    command.stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log);
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

fn beside_tests(top: &Path, lane: &Lane, work: &Path) -> Option<PathBuf> {
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
    Some(work.join("near.diff"))
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

