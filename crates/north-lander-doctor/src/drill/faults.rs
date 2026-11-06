use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use north_landing::land::lane_gates::{self, Judging};
use north_landing::land::queue::{self, Ticket};
use north_landing::land::{link_hold, sheriff, watchdog};
use north_landing::landing::red_check::Failure;

use super::heal;
use super::scratch::{Lander, age, script, write};
use crate::bounds::{Bound, OLDEST_QUEUED_SECS, ROUND_SILENCE_SECS, SLOT_DEADLINE_SECS};
use crate::facts::{self, Facts};

pub struct Fault {
    pub key: &'static str,
    pub name: &'static str,
    pub drill: fn(&mut Lander) -> Result<String, String>,
}

/// Each stuck state the lander met on 2026-11-06, in the order the drill injects them.
pub const FAULTS: &[Fault] = &[
    Fault { key: "held-link", name: "lander link held by a replay that reached no verdict", drill: held_link },
    Fault { key: "debt-gate", name: "debt-ceiling refusal of a lane that pays no debt", drill: debt_gate },
    Fault { key: "full-disk", name: "full disk on a check host", drill: full_disk },
    Fault { key: "wedged-unit", name: "wedged lander unit", drill: wedged_unit },
    Fault { key: "stale-runtime", name: "check refused because its host lacks the runtime", drill: stale_runtime },
    Fault { key: "dead-slot", name: "dead offload slot", drill: dead_slot },
    Fault { key: "flaky-red-main", name: "red main from a flaky test", drill: flaky_red_main },
];

const REPLAYS_LANDED: &str = "#!/bin/sh\n[ \"$1 $2\" = \"land replay\" ] && echo 'replay: verdict landed'\nexit 0\n";
const DISK_MIB: u32 = 64;
const FILL: &str = r#"slot=$HOME/.cache/land-offload/code-drill-main-1
mkdir -p "$slot" && : > "$slot.lock" && head -c "$1" /dev/zero > "$slot/target"
"#;
const LANDER: &str = r#"while :; do touch "$1"; sleep 20; done"#;
const WEDGED: &str = r#"touch "$1" && echo started >> "$1.starts" && exec sleep 3600"#;
const HOLDS_SLOT: &str = r#"exec 9>>"$1.lock" && flock 9 && echo held && exec sleep 3600"#;
const FLAKY: &str = "#!/bin/sh\necho run >> \"$0.runs\"\n[ \"$(wc -l < \"$0.runs\")\" -gt 1 ]\n";

fn checkout(lander: &Lander) -> PathBuf {
    lander.root.join("code/north/main")
}

/// A repository at [`checkout`] cloned from a bare origin in the scratch, with `start` committed; returns that commit.
fn repository(lander: &Lander) -> Result<String, String> {
    let origin = lander.root.join("origin.git");
    fs::create_dir_all(&origin).map_err(|error| format!("{}: {error}", origin.display()))?;
    lander.git(&origin, &["init", "-q", "--bare", "-b", "main"])?;
    let parent = checkout(lander).parent().map(Path::to_path_buf).unwrap_or_default();
    fs::create_dir_all(&parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    lander.git(&parent, &["clone", "-q", &origin.display().to_string(), "main"])?;
    commit(lander, "README", "start")
}

fn commit(lander: &Lander, file: &str, message: &str) -> Result<String, String> {
    let checkout = checkout(lander);
    write(&checkout.join(file), &format!("{message}\n"))?;
    lander.git(&checkout, &["add", "-A"])?;
    lander.git(&checkout, &["commit", "-q", "-m", message])?;
    lander.git(&checkout, &["rev-parse", "HEAD"])
}

fn queue_dir(lander: &Lander) -> PathBuf {
    lander.state().join("north/land-queue/drill")
}

fn ticket(branch: &str, head: &str, verdict: Option<String>) -> Ticket {
    Ticket { branch: branch.to_owned(), head: head.to_owned(), verdict, ..Ticket::default() }
}

fn put(path: &Path, ticket: &Ticket) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    queue::write(path, ticket).map_err(|error| format!("{}: {error}", path.display()))
}

fn micros(seconds_ago: u64) -> u128 {
    u128::from(facts::now().saturating_sub(seconds_ago)) * 1_000_000
}

/// The promotion the doctor's retry starts, run here against the scratch repository: `north runtime promote` runs this same
/// `north land promote` for the lander link after its own update steps, which a scratch lander has none of.
pub fn promote(lander: &Lander) -> Result<String, String> {
    let program = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = Command::new(program).args(["land", "promote", "--shadow-minutes", "0", "--repo"]).arg(checkout(lander)).envs(lander.env()).stdin(Stdio::null()).output().map_err(|error| format!("north land promote: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let said = stdout.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or_default().to_owned();
    if output.status.success() { Ok(said) } else { Err(format!("north land promote exited {}: {said} {}", output.status, String::from_utf8_lossy(&output.stderr).trim())) }
}

fn held_link(lander: &mut Lander) -> Result<String, String> {
    let base = repository(lander)?;
    let landed = commit(lander, "a.txt", "lane a (north#634)")?;
    lander.git(&checkout(lander), &["push", "-q", "origin", "HEAD:refs/heads/main"])?;
    let candidate = commit(lander, "candidate.txt", "candidate (north#634)")?;
    let origin = lander.git(&checkout(lander), &["remote", "get-url", "--push", "origin"])?;
    let round = Ticket { origin: origin.clone(), root: checkout(lander), decided_on: Some(base), ..ticket("lane-a", &landed, Some(format!("landed {landed}"))) };
    put(&queue::directory(&lander.state(), &origin).join("done").join(queue::file_name(1, "lane-a")), &round)?;
    let home = lander.state().join("north");
    for (build, link) in [("held", "lander"), (candidate.as_str(), "current")] {
        script(&home.join("runtimes").join(build).join(north_base::platform::harness::TREE).join("bin/north"), REPLAYS_LANDED)?;
        std::os::unix::fs::symlink(format!("runtimes/{build}"), home.join(link)).map_err(|error| format!("{link}: {error}"))?;
    }
    link_hold::unreached(&home, &candidate, "round lane-a: transient mkdir -p on tom@gex45 did not finish within 20s");
    heal(lander, Bound::NoVerdict, &|now| Facts { link: facts::link_facts(&home, None, Vec::new(), now), ..Facts::default() })
}

fn debt_gate(lander: &mut Lander) -> Result<String, String> {
    repository(lander)?;
    let goals = "[[goal]]\nname = \"landing-projected\"\nunit = \"s\"\nmax = 20\nclass = \"debt\"\nceiling = 60\npays = [\"crates/north-landing/\"]\n";
    write(&checkout(lander).join("goals.toml"), goals)?;
    let base = commit(lander, "landing-projected.txt", "landing-projected reads 283 s, over its 60 s ceiling")?;
    let head = commit(lander, "crates/north-runtime/notes.txt", "a lane that touches only north-runtime (north#634)")?;
    let lane = lane_gates::Lane { worktree: &checkout(lander), base: &base, head: &head };
    match lane_gates::judge(&lane, Judging::Recorded, &lander.root.join("logs"), "drill-lane")? {
        None => Ok("the lane gates admitted a lane that pays no debt while landing-projected reads over its ceiling; debt orders lanes, it refuses none".to_owned()),
        Some(refused) => Err(format!("the lane gates refused a lane that pays no debt: {}", refused.verdict)),
    }
}

fn full_disk(lander: &mut Lander) -> Result<String, String> {
    let mount = format!("mount -t tmpfs -o size={DISK_MIB}m north-drill \"$1\" && echo mounted && exec sleep 3600");
    let holder = lander.hold(&["unshare", "-rm", "--propagation", "private"], &mount, &lander.home())?;
    let place = lander.place(Some(holder));
    facts::shell(&place, FILL, &[&format!("{}M", DISK_MIB - 3)])?;
    heal(lander, Bound::DiskFree, &|_| Facts { places: vec![facts::place_facts(place.clone())], ..Facts::default() })
}

/// Starts a scratch lander unit for the queue that writes its rounds file the way a lander does each round.
fn serve(lander: &mut Lander, name: &str, script: &str) -> Result<(String, PathBuf), String> {
    let rounds = queue_dir(lander).join(watchdog::ROUNDS);
    write(&rounds, "[]\n")?;
    let unit = lander.unit(name);
    lander.start_unit(&unit, script, &rounds)?;
    Ok((unit, rounds))
}

fn wedged_unit(lander: &mut Lander) -> Result<String, String> {
    let queue = queue_dir(lander);
    put(&queue.join(queue::file_name(micros(OLDEST_QUEUED_SECS), "drill-lane")), &ticket("drill-lane", &"0".repeat(40), None))?;
    let (unit, rounds) = serve(lander, "wedged", WEDGED)?;
    let started = rounds.with_extension("json.starts");
    for _ in 0..50 {
        if started.exists() {
            break;
        }
        sleep(Duration::from_millis(100));
    }
    age(&rounds, ROUND_SILENCE_SECS + 300)?;
    heal(lander, Bound::RoundSilence, &|now| Facts { queues: vec![facts::served_queue_facts("drill", &queue, unit.clone(), now)], ..Facts::default() })
}

fn stale_runtime(lander: &mut Lander) -> Result<String, String> {
    let queue = queue_dir(lander);
    let (unit, _) = serve(lander, "lander", LANDER)?;
    let log = lander.root.join("logs/land-check.log");
    write(&log, "safe-push: land check: cargo nextest run --workspace\nnorth land offload: runtime runtimes/0123456789ab is not on this host; NOT pushed.\n")?;
    let refused = ticket("drill-lane", &"0".repeat(40), Some(format!("failed land check cargo nextest exited 1; log: {}", log.display())));
    put(&queue.join("done").join(queue::file_name(micros(60), "drill-lane")), &refused)?;
    heal(lander, Bound::InfrastructureFailures, &|now| Facts { queues: vec![facts::served_queue_facts("drill", &queue, unit.clone(), now)], ..Facts::default() })
}

fn dead_slot(lander: &mut Lander) -> Result<String, String> {
    let slot = lander.home().join(".cache/land-offload/code-drill-main-1");
    fs::create_dir_all(&slot).map_err(|error| format!("{}: {error}", slot.display()))?;
    let pgid = lander.hold(&[], HOLDS_SLOT, &slot)?;
    let beside = |suffix: &str| slot.with_extension(suffix);
    write(&beside("pgid"), &format!("{pgid}\n"))?;
    write(&beside("owner"), "land-check-drill\n")?;
    write(&beside("holder"), "0123456789ab since 13:00:00\n")?;
    age(&beside("holder"), SLOT_DEADLINE_SECS + 60)?;
    let place = lander.place(None);
    heal(lander, Bound::SlotHold, &|_| Facts { places: vec![facts::place_facts(place.clone())], ..Facts::default() })
}

fn flaky_red_main(lander: &mut Lander) -> Result<String, String> {
    let check = lander.root.join("flaky-check");
    script(&check, FLAKY)?;
    let failure = Failure::Cargo("drill::a_test_that_fails_once_under_load".to_owned());
    let run = || Command::new(&check).stdin(Stdio::null()).status().is_ok_and(|status| status.success());
    if run() {
        return Err("the flaky check passed on its first run, so main never went red".to_owned());
    }
    let failing: BTreeSet<Failure> = [failure.clone()].into();
    let request = sheriff::Request { head: "tip".to_owned(), parent: "green".to_owned(), run: "drill".to_owned(), log: String::new(), failing: failing.clone() };
    let lanes = [sheriff::Lane { commit: "tip".to_owned(), message: "an innocent lane (north#634)".to_owned() }];
    let rerun = |_: &sheriff::Lane| Some(if run() { BTreeSet::new() } else { failing.clone() });
    match sheriff::find(&request, "tip", &lanes, &BTreeSet::new(), rerun, rerun) {
        sheriff::Finding::Fixed { .. } => Ok(format!("main went red on {}; the sheriff reran it at the tip, read it passing, and reverted nothing", failure.key())),
        other => Err(format!("the sheriff read a test that fails once as {other:?}")),
    }
}
