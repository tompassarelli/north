use super::canary::{self, Verdict};
use super::deploy;
use super::failure::{Failure, failed};
use super::growth::{Growth, print_north};
use super::history::History;
use super::layout::{self, Layout};
use super::lifecycle::{self, Judge, Judgement};
use super::pin;
use super::verdict;
use super::review_hold::Screen;
use super::walk;
use super::watch;
use super::follow;
use super::probe::{self, Shell};
use super::update;
use north_offload::land_offload;
use crate::platform::{host, process};
use rustix::fs::{FlockOperation, flock};
use serde_json::json;
use std::fs::{self, File};
use std::ops::Not;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const SEEN_KEPT: usize = 50;

struct Landing {
    sha: String,
    subject: String,
}

pub(super) fn settle(layout: &Layout) -> Result<(), Failure> {
    fs::create_dir_all(&layout.state)?;
    let lock = File::create(layout.state.join("runtime-settle.lock"))?;
    if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
        println!("north runtime settle already running");
        return Ok(());
    }
    let promoted = promote_here(layout);
    if let Some(live) = handed_to_live(layout) {
        drop(lock);
        let error = Command::new(layout.runtime(&live).join("north")).args(["runtime", "settle"]).exec();
        return Err(failed(1, format!("could not hand settle to the live runtime {}: {error}", layout::short(&live))));
    }
    let checkout = update::checkout();
    let hosts = fleet_hosts();
    let landing = due_landing(layout, &checkout)?;
    let live = |runtime: &str| landing.as_ref().is_none_or(|landing| runtime == landing.sha || ancestor(&checkout, &landing.sha, runtime));
    let mut records = Vec::new();
    let mut failing = Vec::new();
    let mut broken = Vec::new();
    let judging = landing.as_ref().filter(|landing| !judged(layout, &landing.sha));
    let code_changed = judging.is_none_or(|landing| changes_code(&checkout, &landing.sha));
    let mut history = History::load(layout);
    let mut growth = Growth::load(layout);
    let mut incidents = Vec::new();
    let moved_on = landing.as_ref().is_some_and(|landing| origin_head(&checkout).is_some_and(|head| head != landing.sha));
    let here = walk_and_ship(layout, &hosts, &checkout);
    let others = if canaries_at_once() { judged_at_once(layout, &hosts[1..]) } else { hosts[1..].iter().map(|host| probed(layout, host).0).collect() };
    for mut facts in std::iter::once(here).chain(others) {
        let (problems, window) = judged_problems(&mut facts, landing.as_ref(), &checkout, live, &mut growth);
        println!("north runtime settle: {}: {}", facts.host, verdict::verdict(&facts, &problems, landing.is_some()));
        print_north(&facts, window.as_deref());
        if !problems.is_empty() {
            failing.push(format!("{}: {}", facts.host, problems.join("; ")));
            let inherited = judging.map_or(&[][..], |landing| history.before(&facts.host, &landing.sha, |earlier| ancestor(&checkout, earlier, &landing.sha)));
            let (own, elsewhere) = blame(&problems, moved_on, inherited, code_changed);
            broken.extend(own.is_empty().not().then(|| format!("{}: {}", facts.host, own.join("; "))));
            incidents.extend(elsewhere.is_empty().not().then(|| format!("{}: {}", facts.host, elsewhere.join("; "))));
        }
        if let (Some(runtime), None) = (&facts.runtime, &facts.unreachable) {
            history.note(&facts.host, runtime, &problems);
        }
        records.push(probe::record(&facts, &problems));
    }
    if let Err(error) = history.save().and_then(|()| growth.save()) {
        eprintln!("north runtime settle: could not record each host's problems and process counts: {error}");
    }
    for incident in &incidents {
        println!("north runtime settle: host incident, not this landing's change: {incident}");
    }
    let report = json!({"checkedAt": now(), "landing": landing.as_ref().map(|landing| &landing.sha), "hostIncidents": incidents, "hosts": records});
    fs::write(layout.state.join("runtime-settle.json"), format!("{report:#}\n"))?;
    if let Some(landing) = landing.filter(|landing| !judged(layout, &landing.sha)) {
        judge(layout, &checkout, &landing, &broken);
    }
    outcome(promoted, &failing)
}

/// The follower's result: this host's own update, whatever the other hosts' problems, which are printed and judged against the landing but fail nothing, so a pin held by a trip or a reviewer exits 0 (north#1551).
fn outcome(promoted: Result<(), Failure>, failing: &[String]) -> Result<(), Failure> {
    if !failing.is_empty() {
        println!("north runtime settle: not live and healthy everywhere: {}", failing.join(" | "));
    }
    promoted
}

/// Brings this host to the pinned runtime, then walks its canary and follows the lander link from `walk_and_ship` when it passed, and watches a fresh swap for rising failures.
fn promote_here(layout: &Layout) -> Result<(), Failure> {
    let follow::Followed { here: updated, .. } = follow::update_following_lander(layout, false, false);
    watch::watch(layout);
    updated.map(|_| ()).map_err(|failure| failed(failure.code, format!("{}: {}", host::name(), failure.message)))
}

fn fleet_hosts() -> Vec<Shell> {
    let mut hosts = vec![Shell::Here(host::name())];
    hosts.extend(land_offload::other_destinations().iter().cloned().map(Shell::Ssh));
    hosts
}

/// Probes this host first, so its canary walks the spine on the live runtime, and ships that runtime to the hosts behind it only when it passed; the lander link follows the runtime as soon as that canary passed, while the ship runs.
fn walk_and_ship(layout: &Layout, hosts: &[Shell], checkout: &Path) -> probe::Facts {
    let (facts, judged) = probed(layout, &hosts[0]);
    let live = layout.live_sha();
    let walk = walk::walk_of(layout, &facts.host, live.as_deref(), judged.as_ref().map(|(sha, verdict)| (sha.as_str(), verdict)));
    let follow = walk.is_ok();
    let shipped = std::thread::scope(|scope| {
        let following = follow.then(|| scope.spawn(|| follow::follow_lander(layout)));
        let shipped = walk::ship_if_walked(layout, walk, &Screen::here(checkout), |here, kept| {
            let behind: Vec<&Shell> = hosts[1..].iter().filter(|host| kept.iter().any(|destination| destination == host.name())).collect();
            ship_behind(&behind, here, &layout.runtime(here), checkout);
            Ok(())
        });
        if let Some(handle) = following {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(failure)) => eprintln!("north runtime settle: {}", failure.message),
                Err(_) => eprintln!("north runtime settle: the lander link follow panicked"),
            }
        }
        shipped
    });
    if let Err(failure) = shipped {
        eprintln!("north runtime settle: {}", failure.message);
    }
    facts
}

/// Every other host on its own thread, its canary and then its facts, once every ship has returned: the hosts' canaries probe the hub, so none starts while another host's update is still swapping its runtime. Every thread is joined before this returns, and one that panicked comes back as that host unreachable, never as a host left out of the report.
fn judged_at_once(layout: &Layout, hosts: &[Shell]) -> Vec<probe::Facts> {
    std::thread::scope(|scope| {
        let judging: Vec<_> = hosts.iter().map(|host| scope.spawn(move || probed(layout, host).0)).collect();
        judging.into_iter().zip(hosts).map(|(judged, host)| judged.join().unwrap_or_else(|_| probe::Facts { host: host.name().to_owned(), unreachable: Some("its settle probe panicked".to_owned()), ..probe::Facts::default() })).collect()
    })
}

fn canaries_at_once() -> bool {
    north_base::platform::harness::document(north_base::platform::harness::DEFAULTS).get("settle_remote_canaries_at_once").and_then(|entry| entry.get("value")).and_then(|value| value.as_bool()).unwrap_or(true)
}

fn ship_behind(hosts: &[&Shell], here: &str, runtime: &Path, checkout: &Path) {
    std::thread::scope(|scope| {
        for host in hosts {
            scope.spawn(move || {
                let behind = probe::live_runtime(host).is_none_or(|remote| remote != here && ancestor(checkout, &remote, here));
                if !behind {
                    return;
                }
                if let Err(failure) = deploy::ship_runtime(host.name(), here, runtime) {
                    eprintln!("north runtime settle: {}", failure.message);
                }
            });
        }
    });
}

/// The host's facts after its canary, and the runtime the canary judged with its verdict, which the pin journal records when it walked or rolled back.
fn probed(layout: &Layout, host: &Shell) -> (probe::Facts, Option<(String, Verdict)>) {
    let facts = probe::facts(host);
    let Some(runtime) = facts.runtime.clone().filter(|_| facts.unreachable.is_none()) else {
        return (facts, None);
    };
    let verdict = canary::check(layout, &facts.host, &runtime, &canary::Live { layout, host });
    walk::judged(layout, &facts.host, &runtime, &verdict, &facts.swaps);
    let facts = if verdict == Verdict::RolledBack { probe::facts(host) } else { facts };
    (facts, Some((runtime, verdict)))
}

fn handed_to_live(layout: &Layout) -> Option<String> {
    let (own, _) = super::own_runtime()?;
    layout.live_sha().filter(|live| *live != own && layout::has_programs(&layout.runtime(live)))
}

/// The standing hold that keeps `landing` off the host `facts` describe while its runtime predates the landing, read under the deploy name a hold names that host by.
fn held_off(landing: Option<&Landing>, checkout: &Path, facts: &probe::Facts, live: impl Fn(&str) -> bool) -> Option<String> {
    let landing = landing.filter(|_| facts.runtime.as_deref().is_some_and(|runtime| !live(runtime)))?;
    let deployment = land_offload::deployment();
    let name = deployment.destinations.iter().position(|destination| *destination == facts.host).map_or(facts.host.as_str(), |at| deployment.names[at].as_str());
    Screen::here(checkout).holding(&landing.sha, name)
}

fn judged_problems(facts: &mut probe::Facts, landing: Option<&Landing>, checkout: &Path, live: impl Fn(&str) -> bool, growth: &mut Growth) -> (Vec<String>, Option<String>) {
    facts.held = held_off(landing, checkout, facts, &live);
    let mut problems = probe::problems(facts, live);
    let mut window = None;
    if let (Some(north), None, false) = (&facts.north, &facts.unreachable, probe::installing(facts)) {
        growth.note(&facts.host, now(), north.outlasting());
        window = growth.leak(&facts.host, now());
        problems.extend(window.clone());
    }
    (problems, window)
}

fn due_landing(layout: &Layout, checkout: &Path) -> Result<Option<Landing>, Failure> {
    let seen_path = layout.state.join("runtime-settle-seen");
    let mut seen: Vec<(u64, String)> = fs::read_to_string(&seen_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once(' ').and_then(|(at, sha)| Some((at.parse().ok()?, sha.to_owned()))))
        .collect();
    let head = origin_head(checkout);
    if let Some(head) = head.filter(|head| seen.last().is_none_or(|(_, last)| last != head)) {
        seen.push((now(), head));
        let kept = &seen[seen.len().saturating_sub(SEEN_KEPT)..];
        fs::write(&seen_path, kept.iter().map(|(at, sha)| format!("{at} {sha}\n")).collect::<String>())?;
    }
    let deadline = probe::default_number("landing_live_deadline_sec") as u64;
    let pinned = layout.live_sha();
    let promoted_at = pinned.as_deref().map_or(0, |pinned| promoted_at(&pin::read(&layout.state), pinned));
    let held = |sha: &str| pinned.as_deref().is_some_and(|pinned| sha == pinned || ancestor(checkout, sha, pinned));
    let Some(sha) = due_in(&seen, promoted_at, deadline, now(), held) else {
        return Ok(None);
    };
    let subject = process::run("git", Some(checkout), &["log", "-1", "--format=%s", sha]).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).unwrap_or_default();
    Ok(Some(Landing { sha: sha.to_owned(), subject }))
}

fn promoted_at(events: &[pin::Event], pinned: &str) -> u64 {
    events.iter().rev().find_map(|event| match event {
        pin::Event::Promoted { sha, at, .. } if sha == pinned => u64::try_from(*at).ok(),
        _ => None,
    }).unwrap_or(0)
}

/// The landing settle judges: the newest one seen that the pinned runtime holds, once `deadline` passed since it was seen and since the pin was promoted, since a landing the pin does not hold yet is due nowhere.
fn due_in(seen: &[(u64, String)], promoted_at: u64, deadline: u64, now: u64, held: impl Fn(&str) -> bool) -> Option<&str> {
    seen.iter().rev().find(|(at, sha)| (*at).max(promoted_at) + deadline <= now && held(sha)).map(|(_, sha)| sha.as_str())
}

fn blame<'a>(problems: &'a [String], moved_on: bool, inherited: &[String], code_changed: bool) -> (Vec<&'a str>, Vec<&'a str>) {
    problems.iter().map(String::as_str).partition(|problem| code_changed && !(moved_on && problem.starts_with(probe::LAGS)) && !inherited.iter().any(|known| known == problem))
}

fn changes_code(checkout: &Path, sha: &str) -> bool {
    let Ok(output) = process::run("git", Some(checkout), &["show", "--format=", "--name-only", sha]) else {
        return true;
    };
    String::from_utf8_lossy(&output.stdout).lines().any(|path| !path.starts_with("docs/") && !path.ends_with(".md"))
}

fn origin_head(checkout: &Path) -> Option<String> {
    process::run("git", Some(checkout), &["rev-parse", "--verify", "origin/main^{commit}"]).ok().map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|sha| layout::is_sha(sha))
}

fn judged(layout: &Layout, sha: &str) -> bool {
    lifecycle::state_of::<Judge>(layout, sha).ok().flatten() == Some(Judgement::Judged)
}

fn judge(layout: &Layout, checkout: &Path, landing: &Landing, broken: &[String]) {
    if let Err(failure) = lifecycle::move_to::<Judge>(layout, &landing.sha, Judgement::Judging, "judging the landing by every host's probe") {
        println!("north runtime settle: {} not judged here: {}", layout::short(&landing.sha), failure.message);
        return;
    }
    let (to, cause) = match broken.is_empty() || alarm(checkout, landing, broken) {
        true => (Judgement::Judged, "judged; every alarm it needed was delivered"),
        false => (Judgement::Unjudged, "its alarm did not reach the issue; retried on the next tick"),
    };
    if let Err(failure) = lifecycle::move_to::<Judge>(layout, &landing.sha, to, cause) {
        eprintln!("north runtime settle: {}", failure.message);
    }
}

pub(super) fn ancestor(checkout: &Path, earlier: &str, later: &str) -> bool {
    process::run("git", Some(checkout), &["merge-base", "--is-ancestor", earlier, later]).is_ok_and(|output| output.status.success())
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

fn issue(subject: &str) -> Option<&str> {
    let (_, tail) = subject.rsplit_once("(north#")?;
    let number = tail.strip_suffix(')')?;
    (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())).then_some(number)
}

fn alarm(checkout: &Path, landing: &Landing, failing: &[String]) -> bool {
    let short = layout::short(&landing.sha);
    let text = format!("north landing {short} is not live and healthy on every host {}s after it landed: {}", probe::default_number("landing_live_deadline_sec"), failing.join(" | "));
    let delivered = issue(&landing.subject).is_none_or(|number| {
        let reopened = Command::new("gh").current_dir(checkout).args(["issue", "reopen", number]).status();
        let commented = Command::new("gh").current_dir(checkout).args(["issue", "comment", number, "--body", &text]).status();
        let delivered = reopened.is_ok_and(|status| status.success()) || commented.is_ok_and(|status| status.success());
        if !delivered {
            eprintln!("north runtime settle: could not reopen or comment on north#{number}");
        }
        delivered
    });
    println!("north runtime settle: {text}; north ops watch raises it to the proxy and the landing lane's lead");
    delivered
}

#[cfg(test)]
mod tests {
    use super::{blame, due_in, outcome};
    use super::super::failure::failed;

    #[test]
    fn a_host_held_behind_a_trip_or_a_reviewer_does_not_fail_the_follower_but_a_failed_update_here_does() {
        let held = vec!["tom@cpx62: live runtime 7419083c predates the landing".to_owned()];
        assert!(outcome(Ok(()), &held).is_ok());
        let failure = outcome(Err(failed(1, "whiterabbit: build failed".to_owned())), &[]).err().map(|failure| (failure.code, failure.message));
        assert_eq!(failure, Some((1, "whiterabbit: build failed".to_owned())));
    }

    #[test]
    fn settle_judges_only_a_landing_the_pinned_runtime_holds_once_the_pin_had_its_deadline() {
        let seen = vec![(100, "a".to_owned()), (200, "b".to_owned())];
        assert_eq!(due_in(&seen, 0, 60, 1_000, |sha| sha == "a"), Some("a"));
        assert_eq!(due_in(&seen, 0, 60, 1_000, |_| true), Some("b"));
        assert_eq!(due_in(&seen, 950, 60, 1_000, |_| true), None);
        assert_eq!(due_in(&seen, 900, 60, 1_000, |_| true), Some("b"));
        assert_eq!(due_in(&seen, 0, 60, 1_000, |_| false), None);
    }

    #[test]
    fn a_runtime_lagging_a_newer_main_is_not_this_landings_breakage() {
        let lag = vec!["live runtime 1234abcd predates the landing".to_owned()];
        assert_eq!(blame(&lag, true, &[], true), (vec![], vec![lag[0].as_str()]));
        assert_eq!(blame(&lag, false, &[], true).0, [lag[0].as_str()]);
        let mixed = vec![lag[0].clone(), "north-runtime-follow.service is failed".to_owned()];
        assert_eq!(blame(&mixed, true, &[], true).0, ["north-runtime-follow.service is failed"]);
    }

    #[test]
    fn a_problem_the_previous_runtime_had_or_a_landing_that_changes_no_code_is_a_host_incident() {
        let problems = vec!["north-runtime-follow.service is failed".to_owned(), "unresolved hook path a: /x".to_owned()];
        let (own, elsewhere) = blame(&problems, false, &problems[..1], true);
        assert_eq!((own, elsewhere), (vec!["unresolved hook path a: /x"], vec!["north-runtime-follow.service is failed"]));
        let (own, elsewhere) = blame(&problems, false, &[], false);
        assert!(own.is_empty() && elsewhere.len() == 2);
    }
}
