use super::deploy;
use north_foundation::foundation::failure::{Failure, failed};
use super::growth::{Growth, print_north};
use super::history::History;
use super::layout::{self, Layout};
use super::lifecycle::{self, Judge, Judgement};
use super::pin;
use super::verdict;
use super::review_hold::Screen;
use super::ship;
use super::follow;
use super::probe::{self, Shell};
use super::update;
use north_offload::land_offload;
use north_base::platform::{host, process};
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
    let store = update::trusted_store()?;
    let hosts = fleet_hosts();
    let landing = due_landing(layout, &store)?;
    let live = |runtime: &str| landing.as_ref().is_none_or(|landing| runtime == landing.sha || ancestor(&store, &landing.sha, runtime));
    let mut records = Vec::new();
    let mut failing = Vec::new();
    let mut broken = Vec::new();
    let judging = landing.as_ref().filter(|landing| !judged(layout, &landing.sha));
    let code_changed = judging.is_none_or(|landing| changes_code(&store, &landing.sha));
    let mut history = History::load(layout);
    let mut growth = Growth::load(layout);
    let mut incidents = Vec::new();
    let moved_on = landing.as_ref().is_some_and(|landing| origin_head(&store).is_some_and(|head| head != landing.sha));
    let here = ship_here(layout, &hosts, &store);
    let others = facts_at_once(&hosts[1..]);
    for mut facts in std::iter::once(here).chain(others) {
        let (problems, window) = judged_problems(&mut facts, landing.as_ref(), &store, live, &mut growth);
        println!("north runtime settle: {}: {}", facts.host, verdict::verdict(&facts, &problems, landing.is_some()));
        print_north(&facts, window.as_deref());
        if !problems.is_empty() {
            failing.push(format!("{}: {}", facts.host, problems.join("; ")));
            let inherited = judging.map_or(&[][..], |landing| history.before(&facts.host, &landing.sha, |earlier| ancestor(&store, earlier, &landing.sha)));
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
        judge(layout, &update::checkout(), &landing, &broken);
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

/// Brings this host to the pinned runtime, then follows the lander link and ships from `ship_here`.
fn promote_here(layout: &Layout) -> Result<(), Failure> {
    let follow::Followed { here: updated, .. } = follow::update_following_lander(layout, false);
    updated.map(|_| ()).map_err(|failure| failed(failure.code, format!("{}: {}", host::name(), failure.message)))
}

fn fleet_hosts() -> Vec<Shell> {
    let mut hosts = vec![Shell::Here(host::name())];
    hosts.extend(land_offload::other_destinations().iter().cloned().map(Shell::Ssh));
    hosts
}

/// Reads this host first, ships its live runtime to the hosts behind it and follows the lander link to it while the ship runs.
fn ship_here(layout: &Layout, hosts: &[Shell], store: &Path) -> probe::Facts {
    let facts = probe::facts(&hosts[0]);
    let live = ship::live_here(layout);
    let follow = live.is_ok();
    let shipped = std::thread::scope(|scope| {
        let following = follow.then(|| scope.spawn(|| follow::follow_lander(layout)));
        let shipped = ship::ship_screened(layout, live, &Screen::here(store), |here, kept| {
            let behind: Vec<&Shell> = hosts[1..].iter().filter(|host| kept.iter().any(|destination| destination == host.name())).collect();
            ship_behind(&behind, here, &layout.runtime(here), store);
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

/// Every other host's facts, each on a thread of its own. Every thread is joined before this returns, and one that panicked comes back as that host unreachable, never as a host left out of the report.
fn facts_at_once(hosts: &[Shell]) -> Vec<probe::Facts> {
    std::thread::scope(|scope| {
        let judging: Vec<_> = hosts.iter().map(|host| scope.spawn(move || probe::facts(host))).collect();
        judging.into_iter().zip(hosts).map(|(judged, host)| judged.join().unwrap_or_else(|_| probe::Facts { host: host.name().to_owned(), unreachable: Some("its settle probe panicked".to_owned()), ..probe::Facts::default() })).collect()
    })
}

fn ship_behind(hosts: &[&Shell], here: &str, runtime: &Path, store: &Path) {
    std::thread::scope(|scope| {
        for host in hosts {
            scope.spawn(move || {
                let behind = probe::live_runtime(host).is_none_or(|remote| remote != here && ancestor(store, &remote, here));
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

fn handed_to_live(layout: &Layout) -> Option<String> {
    let (own, _) = super::own_runtime()?;
    layout.live_sha().filter(|live| *live != own && layout::has_programs(&layout.runtime(live)))
}

/// The standing hold that keeps `landing` off the host `facts` describe while its runtime predates the landing, read under the deploy name a hold names that host by.
fn held_off(landing: Option<&Landing>, store: &Path, facts: &probe::Facts, live: impl Fn(&str) -> bool) -> Option<String> {
    let landing = landing.filter(|_| facts.runtime.as_deref().is_some_and(|runtime| !live(runtime)))?;
    holding(store, &landing.sha, &facts.host)
}

/// The standing hold that keeps `head` off `host`, read under the deploy name a hold names that host by.
pub(super) fn holding(store: &Path, head: &str, host: &str) -> Option<String> {
    let deployment = land_offload::deployment();
    let name = deployment.destinations.iter().position(|destination| destination == host).map_or(host, |at| deployment.names[at].as_str());
    Screen::here(store).holding(head, name)
}

fn judged_problems(facts: &mut probe::Facts, landing: Option<&Landing>, store: &Path, live: impl Fn(&str) -> bool, growth: &mut Growth) -> (Vec<String>, Option<String>) {
    facts.held = held_off(landing, store, facts, &live);
    let mut problems = probe::problems(facts, live);
    let mut window = None;
    if let (Some(north), None, false) = (&facts.north, &facts.unreachable, probe::installing(facts)) {
        growth.note(&facts.host, now(), north.outlasting());
        window = growth.leak(&facts.host, now());
        problems.extend(window.clone());
    }
    (problems, window)
}

fn due_landing(layout: &Layout, store: &Path) -> Result<Option<Landing>, Failure> {
    let seen_path = layout.state.join("runtime-settle-seen");
    let mut seen: Vec<(u64, String)> = fs::read_to_string(&seen_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once(' ').and_then(|(at, sha)| Some((at.parse().ok()?, sha.to_owned()))))
        .collect();
    let head = origin_head(store);
    if let Some(head) = head.filter(|head| seen.last().is_none_or(|(_, last)| last != head)) {
        seen.push((now(), head));
        let kept = &seen[seen.len().saturating_sub(SEEN_KEPT)..];
        fs::write(&seen_path, kept.iter().map(|(at, sha)| format!("{at} {sha}\n")).collect::<String>())?;
    }
    let deadline = probe::default_number("landing_live_deadline_sec") as u64;
    let pinned = layout.live_sha();
    let promoted_at = pinned.as_deref().map_or(0, |pinned| promoted_at(&pin::read(&layout.state), pinned));
    let held = |sha: &str| pinned.as_deref().is_some_and(|pinned| sha == pinned || ancestor(store, sha, pinned));
    let Some(sha) = due_in(&seen, promoted_at, deadline, now(), held) else {
        return Ok(None);
    };
    let subject = process::run("git", Some(store), &["log", "-1", "--format=%s", sha]).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).unwrap_or_default();
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

fn changes_code(store: &Path, sha: &str) -> bool {
    let Ok(output) = process::run("git", Some(store), &["show", "--format=", "--name-only", sha]) else {
        return true;
    };
    String::from_utf8_lossy(&output.stdout).lines().any(|path| !path.starts_with("docs/") && !path.ends_with(".md"))
}

fn origin_head(store: &Path) -> Option<String> {
    process::run("git", Some(store), &["rev-parse", "--verify", "origin/main^{commit}"]).ok().map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|sha| layout::is_sha(sha))
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

pub(super) fn ancestor(store: &Path, earlier: &str, later: &str) -> bool {
    process::run("git", Some(store), &["merge-base", "--is-ancestor", earlier, later]).is_ok_and(|output| output.status.success())
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
    println!("north runtime settle: {text}; north ops watch raises it to the coordinator and the landing lane's lead");
    delivered
}

#[cfg(test)]
mod tests {
    use super::outcome;
    use north_foundation::foundation::failure::failed;

    #[test]
    fn a_host_held_behind_a_trip_or_a_reviewer_does_not_fail_the_follower_but_a_failed_update_here_does() {
        let held = vec!["tom@cpx62: live runtime 7419083c predates the landing".to_owned()];
        assert!(outcome(Ok(()), &held).is_ok());
        let failure = outcome(Err(failed(1, "whiterabbit: build failed".to_owned())), &[]).err().map(|failure| (failure.code, failure.message));
        assert_eq!(failure, Some((1, "whiterabbit: build failed".to_owned())));
    }

}
