use super::*;
use super::probes::Probe;
use super::lifecycle::{Promoting, Promotion};
use std::process::Command;
use std::sync::Mutex;
use std::fs;
use std::os::unix::fs::symlink;

const PREVIOUS: &str = "1111111111111111111111111111111111111111";
const PROMOTED: &str = "2222222222222222222222222222222222222222";

struct Fleet<'a> {
    layout: &'a Layout,
    failing: Option<(&'static str, Ran)>,
    replaced: Option<(&'static str, Ran)>,
    launchable: Vec<String>,
    reaches_hub: bool,
    settles: bool,
    landers: Option<probes::Landers>,
    depbox_host: Option<std::path::PathBuf>,
    probed: Mutex<Vec<String>>,
    alarms: Mutex<Vec<String>>,
    pages: Mutex<Vec<(String, String)>>,
}

impl Machine for Fleet<'_> {
    fn probe(&self, probe: &Probe) -> Ran {
        self.probed.lock().unwrap().push(probe.name.clone());
        if let Some(home) = self.depbox_host.as_ref().filter(|_| probe.name.contains("depbox")) {
            let output = Command::new(bash_program()).arg("-c").arg(&probe.script).env("HOME", home).env("TMPDIR", home.join("tmp")).env_remove("XDG_STATE_HOME").env_remove("NORTH_RUNTIME_LINK").env_remove(north_base::platform::session_scope::REMOVAL_ROOTS).output().unwrap();
            return classify(output.status.code(), format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)));
        }
        let failing = if probe.script.contains(PREVIOUS) { &self.replaced } else { &self.failing };
        match failing {
            Some((name, Ran::Exited { code, log })) if probe.name.contains(name) => Ran::Exited { code: *code, log: log.clone() },
            Some((name, Ran::Unrun(why))) if probe.name.contains(name) => Ran::Unrun(why.clone()),
            Some((name, Ran::HostFault(why))) if probe.name.contains(name) => Ran::HostFault(why.clone()),
            _ => Ran::Exited { code: 0, log: String::new() },
        }
    }

    fn roll_back(&self, sha: &str) -> Rollback {
        Live { layout: self.layout, host: &super::super::probe::Shell::Here("here".to_owned()) }.roll_back(sha)
    }

    fn alarm(&self, text: &str) {
        self.alarms.lock().unwrap().push(text.to_owned());
    }

    fn page(&self, key: &str, text: &str) {
        self.pages.lock().unwrap().push((key.to_owned(), text.to_owned()));
    }

    fn rollback_target(&self, sha: &str) -> Option<String> {
        Live { layout: self.layout, host: &super::super::probe::Shell::Here("here".to_owned()) }.rollback_target(sha)
    }

    fn launchable_providers(&self) -> Vec<String> {
        self.launchable.clone()
    }

    fn sessions_reach_hub(&self, _host: &str) -> bool {
        self.reaches_hub
    }

    fn settles_fleet(&self, _host: &str) -> bool {
        self.settles
    }

    fn lander_repositories(&self) -> Option<probes::Landers> {
        self.landers.clone()
    }
}

struct Host {
    _dir: tempfile::TempDir,
    layout: Layout,
}

impl Host {
    fn promoted() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout { state: dir.path().to_path_buf() };
        for sha in [PREVIOUS, PROMOTED] {
            fs::create_dir_all(layout.runtime(sha)).unwrap();
            for program in layout::PROGRAMS {
                fs::write(layout.runtime(sha).join(program), "").unwrap();
            }
        }
        symlink(layout::link(PROMOTED), layout.current()).unwrap();
        lifecycle::move_to::<Promoting>(&layout, PREVIOUS, Promotion::Superseded, "replaced by the promoted runtime").unwrap();
        lifecycle::move_to::<Promoting>(&layout, PROMOTED, Promotion::Live, "passed its health check and agents sync").unwrap();
        lifecycle::move_to::<Canarying>(&layout, &format!("nexus/{PREVIOUS}"), Canary::Passed, "all core-loop probes exited 0").unwrap();
        Self { _dir: dir, layout }
    }

    fn fleet(&self, failing: Option<(&'static str, Ran)>) -> Fleet<'_> {
        Fleet { layout: &self.layout, failing, replaced: None, launchable: super::probes::PROVIDERS.iter().map(|provider| (*provider).to_owned()).collect(), reaches_hub: true, settles: false, landers: None, depbox_host: None, probed: Mutex::new(Vec::new()), alarms: Mutex::new(Vec::new()), pages: Mutex::new(Vec::new()) }
    }

    fn settling(&self, failing: Option<(&'static str, Ran)>) -> Fleet<'_> {
        Fleet { settles: true, ..self.fleet(failing) }
    }

    fn fleet_without(&self, provider: &str) -> Fleet<'_> {
        let mut fleet = self.fleet(None);
        fleet.launchable.retain(|launchable| launchable != provider);
        fleet
    }

    fn current(&self) -> std::path::PathBuf {
        fs::read_link(self.layout.current()).unwrap()
    }
}

fn exited(code: u8, log: &str) -> Ran {
    Ran::Exited { code, log: log.to_owned() }
}

fn bash_program() -> std::path::PathBuf {
    let found = Command::new("/bin/sh").args(["-c", "command -v bash"]).output().unwrap();
    std::path::PathBuf::from(String::from_utf8(found.stdout).unwrap().trim())
}

#[test]
fn a_promotion_that_breaks_a_core_loop_probe_is_rolled_back_with_an_alarm_naming_the_probe() {
    let host = Host::promoted();
    let fleet = host.fleet(Some(("reaches gex45 through", exited(255, "ssh: Could not resolve hostname gex45\n"))));
    assert_eq!(check(&host.layout, "nexus", PROMOTED, &fleet), Verdict::RolledBack);
    assert_eq!(fleet.probed.lock().unwrap().len(), 7);
    assert_eq!(host.current(), layout::link(PREVIOUS));
    assert_eq!(lifecycle::state_of::<Promoting>(&host.layout, PROMOTED).unwrap(), Some(Promotion::RolledBack));
    let alarms = fleet.alarms.lock().unwrap();
    assert_eq!(alarms.len(), 1);
    assert!(alarms[0].contains("probe `a sandboxed worker shell reaches gex45 through north capacity offload` exited 255"), "{}", alarms[0]);
    assert!(alarms[0].contains("Could not resolve hostname gex45"), "{}", alarms[0]);
    assert!(alarms[0].contains("rolled back to 11111111"), "{}", alarms[0]);
    assert!(matches!(update::hold::verdict(&host.layout, PROMOTED, true).unwrap(), update::hold::Verdict::Held(_)));
    assert_eq!(lifecycle::state_of::<Canarying>(&host.layout, &format!("nexus/{PROMOTED}")).unwrap(), Some(Canary::RolledBack));
}

#[test]
fn a_runtime_whose_depbox_cannot_exec_north_is_rolled_back_by_the_canary() {
    const HIDDEN: &str = "#!/bin/sh\n[ \"$1 $2 $3 $4\" = 'depbox run -- true' ] || exit 0\ntop=$(git rev-parse --show-toplevel) || exit 2\ncase \"${NORTH_WORKER_REMOVAL_ROOTS:-}\" in\n*\"\\\"$top\\\"\"*) echo \"bwrap: execvp $0: No such file or directory\" >&2; exit 1 ;;\nesac\nexit 0\n";
    let host = Host::promoted();
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".local/state")).unwrap();
    fs::create_dir(home.path().join("tmp")).unwrap();
    symlink(&host.layout.state, home.path().join(".local/state/north")).unwrap();
    north_base::foundation::shim::write_executable(&host.layout.runtime(PROMOTED).join("north"), HIDDEN);
    north_base::foundation::shim::write_executable(&host.layout.runtime(PREVIOUS).join("north"), "#!/bin/sh\nexit 0\n");
    let fleet = Fleet { depbox_host: Some(home.path().to_path_buf()), ..host.fleet(None) };

    assert_eq!(check(&host.layout, "whiterabbit", PROMOTED, &fleet), Verdict::RolledBack, "on 2026-11-10 a runtime whose depbox hid the north it execs for a worker command went live on whiterabbit and refused every landing through it for about an hour");

    assert_eq!(host.current(), layout::link(PREVIOUS));
    let alarms = fleet.alarms.lock().unwrap();
    assert_eq!(alarms.len(), 1);
    assert!(alarms[0].contains("probe `a worker command's depbox run execs the judged north` exited 1"), "{}", alarms[0]);
    assert!(alarms[0].contains("/north: No such file or directory"), "{}", alarms[0]);
    assert!(alarms[0].contains("rolled back to 11111111"), "{}", alarms[0]);
    assert_eq!(fs::read_dir(home.path().join("tmp")).unwrap().count(), 0, "the probe leaves its scratch checkout behind");
}

#[test]
fn the_hub_reached_as_user_at_host_probes_its_fetch_and_never_ssh_to_itself() {
    let host = Host::promoted();
    let fleet = host.fleet(None);
    assert_eq!(check(&host.layout, "tom@gex45", PROMOTED, &fleet), Verdict::Passed);
    let probed = fleet.probed.lock().unwrap();
    assert!(probed.iter().all(|name| !name.contains("reaches gex45")), "{probed:?}");
    assert!(probed.iter().any(|name| name.contains("own ssh reaches no host")), "{probed:?}");
    assert!(probed.iter().any(|name| name.contains("gex45 fetches")), "{probed:?}");
    assert!(probed.iter().all(|name| !name.contains("--test")), "{probed:?}");
}

#[test]
fn a_runtime_that_lacks_the_probe_subcommand_is_unrun_not_rolled_back() {
    let host = Host::promoted();
    let fleet = host.fleet(Some(("opencode", classify(Some(64), "usage: north runtime ...\n".to_owned()))));
    assert!(matches!(check(&host.layout, "nexus", PROMOTED, &fleet), Verdict::Unrun(why) if why.contains("exit 64")));
    assert_eq!(host.current(), layout::link(PROMOTED));
    assert!(fleet.alarms.lock().unwrap().is_empty());
}

#[test]
fn the_launch_probes_scratch_root_carries_the_ledger_north_fleet_opens() {
    let scratch = tempfile::tempdir().unwrap();

    let (state, cwd) = launch::scratch_root(scratch.path()).unwrap();

    assert!(cwd.is_dir());
    let ledger = rusqlite::Connection::open_with_flags(state.join("threads.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
    let events: i64 = ledger.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'events'", [], |row| row.get(0)).unwrap();
    assert_eq!(events, 1);
}

#[test]
fn a_host_that_cannot_mint_its_push_token_is_reported_once_without_rolling_the_runtime_back() {
    let host = Host::promoted();
    let fleet = host.fleet(Some(("lander mints its push token", Ran::HostFault("this host cannot mint the agent push token for /home/tom/code/north/main/.safe-push by the lander program or the broker".to_owned()))));
    assert!(matches!(check(&host.layout, "nexus", PROMOTED, &fleet), Verdict::Unrun(why) if why.contains("host fault")));
    assert_eq!(host.current(), layout::link(PROMOTED));
    assert_eq!(lifecycle::state_of::<Canarying>(&host.layout, &format!("nexus/{PROMOTED}")).unwrap(), Some(Canary::Unjudged));
    let alarms = fleet.alarms.lock().unwrap();
    assert_eq!(alarms.len(), 1);
    assert!(alarms[0].contains("nexus") && alarms[0].contains("host fault") && alarms[0].contains("stays live"), "{}", alarms[0]);
}

#[test]
fn a_host_whose_lander_uses_the_host_token_passes_the_token_probe() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let repo = home.join("code/north/main");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join(".safe-push"), "[landing]\n\tpush-app = agent\n").unwrap();
    let token = home.join("host-token");
    north_base::foundation::shim::write_executable(&token, "#!/bin/sh\n[ \"$1\" = --app ] || exit 9\nprintf 'ghs_host_token\\n'\n");
    let lander = home.join("lander");
    north_base::foundation::shim::write_executable(&lander, "#!/bin/sh\nread line\n");
    let mut lander = Command::new(&lander).args(["land", "serve", "--app-token"]).arg(&token).current_dir(&repo).stdin(std::process::Stdio::piped()).spawn().unwrap();
    let bin = home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    north_base::foundation::shim::write_executable(&bin.join("pgrep"), format!("#!/bin/sh\necho {}\n", lander.id()));
    let probe = Command::new("bash")
        .arg("-c")
        .arg(super::probes::lander_tokens("north"))
        .env("HOME", home)
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap();
    let listing = Command::new("bash")
        .arg("-c")
        .arg(super::probes::repositories_script())
        .env("HOME", home)
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap();
    let _ = lander.kill();
    let _ = lander.wait();
    assert!(probe.status.success(), "{}", String::from_utf8_lossy(&probe.stderr));
    assert!(listing.status.success(), "{}", String::from_utf8_lossy(&listing.stderr));
    assert!(String::from_utf8_lossy(&listing.stdout).contains(&format!("served\t{}", repo.join(".safe-push").display())), "{}", String::from_utf8_lossy(&listing.stdout));
    assert!(!String::from_utf8_lossy(&probe.stdout).contains("ghs_host_token") && !String::from_utf8_lossy(&probe.stderr).contains("ghs_host_token"));
}

#[test]
fn a_host_with_a_provider_binary_but_no_launch_route_is_not_probed_for_it() {
    let plan = probes::core_loop("nexus", PROMOTED, &["claude".to_owned()], true, false, None);
    assert_eq!(plan.not_applicable, ["opencode", "pi"]);
    assert!(plan.probes.iter().any(|probe| probe.name.contains("claude")));
    assert!(plan.probes.iter().all(|probe| !probe.name.contains("opencode") && !probe.name.contains("pi")));

    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    north_base::foundation::shim::write_executable(&bin.join("opencode"), "#!/bin/sh\nexit 0\n");
    let listed = || {
        let output = Command::new(bash_program()).arg("-c").arg(probes::launchable_script()).env("HOME", home.path()).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let binary_only = listed();
    assert!(!binary_only.contains("opencode"), "a host that carries the opencode binary but not the Go key kills the worker at its first turn, so it cannot launch the probe: {binary_only:?}");
    let key = home.path().join(".local/state/north/accounts/opencode/go.key");
    fs::create_dir_all(key.parent().unwrap()).unwrap();
    fs::write(&key, "go-key\n").unwrap();
    assert!(listed().contains("opencode"), "a host that holds the Go key launches the probe");

    let host = Host::promoted();
    let fleet = host.fleet_without("opencode");
    assert_eq!(check(&host.layout, "nexus", PROMOTED, &fleet), Verdict::Passed);
    let probed = fleet.probed.lock().unwrap();
    assert!(probed.iter().any(|name| name.contains("claude")), "{probed:?}");
    assert!(probed.iter().all(|name| !name.contains("opencode")), "{probed:?}");
}

#[test]
fn a_host_without_a_running_lander_skips_the_token_probe() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let repo = home.join("code/north/main");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join(".safe-push"), "[landing]\n\tpush-app = agent\n").unwrap();
    let bin = root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    north_base::foundation::shim::write_executable(&bin.join("pgrep"), "#!/bin/sh\nexit 1\n");
    let listing = Command::new(bash_program())
        .arg("-c")
        .arg(probes::repositories_script())
        .env("HOME", &home)
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap();
    assert!(listing.status.success(), "{}", String::from_utf8_lossy(&listing.stderr));
    let landers = probes::parse_repositories(&String::from_utf8_lossy(&listing.stdout));
    assert!(landers.served.is_empty(), "{landers:?}");
    assert_eq!(landers.unserved, [repo.join(".safe-push").to_string_lossy().into_owned()]);

    let plan = probes::core_loop("nexus", PROMOTED, &[], true, false, Some(&landers));
    assert!(plan.probes.iter().all(|probe| !probe.name.contains("lander mints")), "{:?}", plan.probes.iter().map(|probe| &probe.name).collect::<Vec<_>>());
    assert!(plan.not_applicable.iter().any(|reason| reason.contains("no running lander serves")), "{:?}", plan.not_applicable);

    let host = Host::promoted();
    let mut fleet = host.fleet(None);
    fleet.landers = Some(landers);
    assert_eq!(check(&host.layout, "nexus", PROMOTED, &fleet), Verdict::Passed);
    assert!(fleet.probed.lock().unwrap().iter().all(|name| !name.contains("lander mints")));
}

#[test]
fn a_rollback_target_walked_before_the_rollback_is_not_judged_again() {
    let host = Host::promoted();
    let failing = host.fleet(Some(("opencode", exited(1, "a test launch needs NORTH_TEST_ROOT\n"))));
    assert_eq!(check(&host.layout, "nexus", PROMOTED, &failing), Verdict::RolledBack);
    let fleet = host.fleet(Some(("opencode", exited(1, "still failing\n"))));
    assert_eq!(check(&host.layout, "nexus", PREVIOUS, &fleet), Verdict::Skipped);
    assert!(fleet.probed.lock().unwrap().is_empty());
    assert_eq!(host.current(), layout::link(PREVIOUS));
}


#[test]
fn a_host_that_launches_no_sessions_is_not_probed_for_a_worker_shell_reaching_the_hub() {
    let host = Host::promoted();
    let mut fleet = host.fleet(None);
    fleet.reaches_hub = false;
    assert_eq!(check(&host.layout, "tom@cpx62", PROMOTED, &fleet), Verdict::Passed);
    let probed = fleet.probed.lock().unwrap();
    assert!(probed.iter().all(|name| !name.contains("reaches gex45")), "{probed:?}");
    assert!(probed.iter().any(|name| name.contains("own ssh reaches no host")), "{probed:?}");
    assert!(probed.iter().any(|name| name.contains("claude")), "{probed:?}");
}

#[test]
fn a_canary_probe_answered_by_a_provider_limit_retries_on_the_next_account() {
    let weekly = "canary-claude-1 on claude:c answered its first turn with 'You've hit your weekly limit · resets Nov 12, 7pm'; claude:c stays unavailable";
    assert!(launch::refused(weekly) && launch::refused("Not logged in · Please run /login") && launch::refused("Your credit balance is too low"));
    assert!(!launch::refused("north fleet launch failed: the runtime exited 1"));

    let listing = "  claude:c available: 5h 10%\n  claude:d available: 5h 36%\n  claude:api:gmail available: $197\nwr.md: target whiterabbit claude:c (5h 10%)\n";
    let accounts: Vec<Option<String>> = launch::candidates(listing).into_iter().map(Some).collect();
    assert_eq!(accounts, [Some("c".to_owned()), Some("d".to_owned())]);

    let mut tried = Vec::new();
    let verdict = launch::judge("claude", &accounts, |account, _| {
        tried.push(account.unwrap().to_owned());
        Ok(if account == Some("c") { launch::Attempt::Refused(weekly.to_owned()) } else { launch::Attempt::Answered })
    });
    assert!(verdict.is_ok() && tried == ["c", "d"], "a provider limit on the first account must hand the probe to the next: {verdict:?} {tried:?}");

    let verdict = launch::judge("claude", &accounts, |_, _| Ok(launch::Attempt::Refused(weekly.to_owned())));
    assert!(verdict.is_ok(), "every account refused, so the probe is not judged, never failed: {verdict:?}");
    let broken = launch::judge("claude", &accounts, |_, _| Err(crate::runtime::failure::failed(1, "boom")));
    assert!(broken.is_err(), "an answer that is no refusal still fails the probe");
}

#[test]
fn a_probe_answer_is_judged_by_a_planted_token_not_by_the_wording_of_a_heading() {
    let token = "north-canary-0123456789abcdef".to_owned();
    let planted = launch::Evidence::Token(token.clone());
    assert_eq!(launch::carries(&format!("Tom\u{2019}s policy\n{token}"), &planted), Ok(()), "455c0e5c: a typographic apostrophe");
    assert_eq!(launch::carries(&format!("Act and finish {}  ", token.to_uppercase()), &planted), Ok(()), "12f6d9b6: the first section heading");
    assert!(launch::carries("Act and finish", &planted).is_err(), "a worker that never saw the policy has no token");
    let headings = launch::Evidence::Headings(vec!["Tom's policy".to_owned(), "Act and finish".to_owned()]);
    assert_eq!(launch::carries("Tom\u{2019}s policy", &headings), Ok(()));
    assert_eq!(launch::carries("Act and finish", &headings), Ok(()));
    assert!(launch::carries("ok", &headings).is_err());
    assert_eq!(launch::carries("ok", &launch::Evidence::Nothing), Ok(()));
}

#[test]
fn a_worker_declining_the_planted_token_as_a_secret_is_no_verdict_on_the_runtime() {
    let token = "north-canary-0123456789abcdef".to_owned();
    let planted = launch::Evidence::Token(token.clone());
    let line = launch::planted_line(&token);
    assert!(line.contains(&token) && line.contains("not a secret"), "{line}");
    let recorded = "I won't copy the canary token into output, because it is a secret marker and the policy says to keep secrets out of output.";
    assert!(launch::carries(recorded, &planted).is_err() && launch::declined_as_secret(recorded));
    assert!(matches!(launch::judged("canary-opencode-1", recorded, &planted), Ok(launch::Attempt::Refused(_))), "cpx62, runtime 889cc6f9: the refusal is no failure of the code under test");
    for refusal in [
        "I won't paste the canary token. It is a leak-detection marker embedded in the policy text; the standing policy requires keeping secrets out of output.",
        "I won't print the token itself. Here is its SHA-256 fingerprint instead: 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        "It's a tripwire planted to detect exactly this kind of echo, so I'm not going to repeat it.",
    ] {
        assert!(matches!(launch::judged("canary-opencode-1", refusal, &planted), Ok(launch::Attempt::Refused(_))), "08e7a19f on cpx62 and cpx62b: {refusal}");
    }
    assert!(launch::judged("canary-opencode-1", "I do not see any policy text.", &planted).is_err(), "a worker that never saw the policy still fails");
    assert!(matches!(launch::judged("canary-opencode-1", &token, &planted), Ok(launch::Attempt::Answered)));
}

#[test]
fn a_probe_still_working_at_its_deadline_is_no_verdict() {
    let name = "canary-opencode-1";
    assert!(matches!(launch::at_deadline(name, true, "status working"), Ok(launch::Attempt::Busy(_))), "03da559f at laptop load 150: a worker still working has not judged the runtime");
    let busy = launch::judge("opencode", &[None], |_, _| launch::at_deadline(name, true, "status working"));
    assert!(busy.is_ok(), "no verdict means the probe exits 0 and the canary rolls nothing back: {busy:?}");
    let exited = launch::judge("opencode", &[None], |_, _| launch::at_deadline(name, false, "status exited"));
    assert!(exited.is_err(), "a worker that stopped without answering still fails the runtime");
}

#[test]
fn a_host_with_no_previous_runtime_holds_a_failing_promotion_instead_of_failing_its_rollback() {
    let host = Host::promoted();
    fs::remove_dir_all(host.layout.runtime(PREVIOUS)).unwrap();
    let fleet = host.fleet(Some(("reaches gex45 through", exited(255, "ssh: Could not resolve hostname gex45\n"))));
    let verdict = check(&host.layout, "cpx62", PROMOTED, &fleet);
    assert!(matches!(verdict, Verdict::Unrun(ref why) if why.contains("nothing to roll back")), "{verdict:?}");
    assert_eq!(host.current(), layout::link(PROMOTED));
    assert!(fleet.alarms.lock().unwrap().iter().all(|alarm| !alarm.contains("could not roll back")));
}

#[test]
fn a_runtime_that_fails_verify_north_or_the_scratch_landing_is_not_shipped() {
    for failing in ["verify-north", "scratch landing"] {
        let host = Host::promoted();
        let fleet = host.settling(Some((failing, exited(1, "the spine walk failed"))));
        assert_eq!(check(&host.layout, "whiterabbit", PROMOTED, &fleet), Verdict::RolledBack, "{failing} did not roll the runtime back");
        assert_eq!(host.current(), layout::link(PREVIOUS), "{failing} left the failed runtime live");
        let walked = super::super::walk::walk_of(&host.layout, "whiterabbit", host.layout.live_sha().as_deref(), Some((PROMOTED, &Verdict::RolledBack)));
        let shipped = std::cell::RefCell::new(Vec::new());
        let _ = super::super::walk::ship_if_walked(walked, |sha| {
            shipped.borrow_mut().push(sha.to_owned());
            Ok(())
        });
        assert!(!shipped.borrow().iter().any(|sha| sha == PROMOTED), "{failing} failed yet {PROMOTED} shipped: {:?}", shipped.borrow());
    }
    let settling = probes::core_loop("whiterabbit", PROMOTED, &[], true, true, None);
    let names: Vec<&str> = settling.probes.iter().map(|probe| probe.name.as_str()).collect();
    assert!(names.iter().any(|name| name.contains("verify-north")), "{names:?}");
    assert!(names.iter().any(|name| name.contains("scratch landing")), "{names:?}");
    let elsewhere = probes::core_loop("nexus", PROMOTED, &[], true, false, None);
    let names: Vec<&str> = elsewhere.probes.iter().map(|probe| probe.name.as_str()).collect();
    assert!(names.iter().all(|name| !name.contains("verify-north") && !name.contains("scratch landing")), "{names:?}");
}

#[test]
fn a_launching_host_whose_sessions_reach_no_other_host_is_not_probed_for_a_worker_shell_reaching_the_hub() {
    let config: toml_edit::DocumentMut = "[hosts.cpx62]\nlaunches = true\nreaches = []\n[hosts.whiterabbit]\nreaches = [\"gex45\", \"cpx62\"]\n[hosts.nexus]\n[hosts.cpx62b]\nlaunches = false\n".parse().unwrap();
    assert!(!probes::sessions_reach_hub(&config, "cpx62"), "on 2026-11-10 the hub probe failed on a host declared to reach no other host and rolled back the laptop's runtime");
    assert!(probes::sessions_reach_hub(&config, "whiterabbit"));
    assert!(probes::sessions_reach_hub(&config, "nexus"));
    assert!(!probes::sessions_reach_hub(&config, "cpx62b"));
}

#[test]
fn a_rollback_lands_only_on_a_runtime_whose_canary_passed() {
    const UNJUDGED: &str = "2222222222222222222222222222222222222222";
    const FAILING: &str = "3333333333333333333333333333333333333333";
    let host = Host::promoted();
    for sha in [UNJUDGED, FAILING] {
        fs::create_dir_all(host.layout.runtime(sha)).unwrap();
        for program in layout::PROGRAMS {
            fs::write(host.layout.runtime(sha).join(program), "").unwrap();
        }
    }
    lifecycle::move_to::<Promoting>(&host.layout, UNJUDGED, Promotion::Superseded, "replaced").unwrap();
    let target = lifecycle::previous(&host.layout, FAILING, |sha| layout::has_programs(&host.layout.runtime(sha))).unwrap();
    assert_eq!(target.as_deref(), Some(PREVIOUS), "on 2026-11-10 a rollback landed on an unjudged runtime and the regression window began there");
}

#[test]
fn a_runtime_that_goes_live_by_promote_settle_or_update_gets_a_canary_verdict() {
    let host = Host::promoted();
    let name = "whiterabbit";
    let fleet = host.fleet(None);
    assert!(!update::judge_outgoing(&host.layout, name, PROMOTED, &fleet), "a passing outgoing runtime lets the promotion go on");
    assert_eq!(lifecycle::state_of::<Canarying>(&host.layout, &format!("{name}/{PROMOTED}")).unwrap(), Some(Canary::Passed), "on 2026-11-10 six runtimes went live on whiterabbit and were replaced with no canary verdict");
    let failing = Host::promoted();
    let fleet = failing.fleet(Some(("opencode", exited(1, "a test launch needs NORTH_TEST_ROOT\n"))));
    assert!(update::judge_outgoing(&failing.layout, name, PROMOTED, &fleet), "a failing outgoing runtime rolls back, so the promotion waits for the next pass");
    assert_eq!(failing.current(), layout::link(PREVIOUS));
}

#[test]
fn an_incoming_runtime_that_fails_where_the_outgoing_also_fails_stays_live() {
    const WALK: &str = "verify-north walks the spine";
    let stops = |step: u32| exited(1, &format!("step {step} FAIL     907 ms  the walk's check\nverify-north FAILED at step {step}  total 24571 ms\n"));
    let canary = |host: &Host, sha: &str| lifecycle::state_of::<Canarying>(&host.layout, &format!("whiterabbit/{sha}")).unwrap();

    let shared = Host::promoted();
    lifecycle::move_to::<Canarying>(&shared.layout, &format!("whiterabbit/{PREVIOUS}"), Canary::Trusted, "the rollback target after 768b570e failed; trusted, not re-judged").unwrap();
    let mut fleet = shared.settling(Some((WALK, stops(4))));
    fleet.replaced = Some((WALK, stops(4)));
    assert_eq!(check(&shared.layout, "whiterabbit", PROMOTED, &fleet), Verdict::Kept, "on 2026-11-10 every runtime failing step 4 was rolled back onto 43ec2f7c, which failed step 4 too and had never been walked");
    assert_eq!(shared.current(), layout::link(PROMOTED));
    assert_eq!(canary(&shared, PROMOTED), Some(Canary::Kept));
    assert_eq!(canary(&shared, PREVIOUS), Some(Canary::Kept), "the outgoing runtime's own failure is on record");
    assert!(fleet.alarms.lock().unwrap().is_empty());
    let pages = fleet.pages.lock().unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].0, "runtime-canary-shared:whiterabbit:verify-north walks the spine on the promoted runtime step 4");

    let passing = Host::promoted();
    lifecycle::move_to::<Canarying>(&passing.layout, &format!("whiterabbit/{PREVIOUS}"), Canary::Trusted, "the rollback target after 768b570e failed; trusted, not re-judged").unwrap();
    let fleet = passing.settling(Some((WALK, stops(4))));
    assert_eq!(check(&passing.layout, "whiterabbit", PROMOTED, &fleet), Verdict::RolledBack, "a runtime that fails a step the outgoing passes rolls back");
    assert_eq!(passing.current(), layout::link(PREVIOUS));
    assert_eq!(canary(&passing, PREVIOUS), Some(Canary::Passed));

    let earlier = Host::promoted();
    lifecycle::move_to::<Canarying>(&earlier.layout, &format!("whiterabbit/{PREVIOUS}"), Canary::Kept, "probe `verify-north walks the spine on the promoted runtime` exited 1; log tail: step 4 FAIL | verify-north FAILED at step 4  total 24571 ms on whiterabbit for runtime 11111111").unwrap();
    let fleet = earlier.settling(Some((WALK, stops(2))));
    let planned = probes::core_loop("whiterabbit", PROMOTED, &fleet.launchable, true, true, None).probes.len();
    assert_eq!(check(&earlier.layout, "whiterabbit", PROMOTED, &fleet), Verdict::RolledBack, "a runtime that stops at an earlier step than the outgoing rolls back");
    assert_eq!(earlier.current(), layout::link(PREVIOUS));
    assert_eq!(fleet.probed.lock().unwrap().len(), planned, "an outgoing walk on record is read, not walked again");
}

#[test]
fn a_probe_worker_follows_the_judged_runtime_while_the_link_still_names_the_live_one() {
    let home = tempfile::tempdir().unwrap();
    let state = home.path().join(".local/state/north");
    let judged = state.join("runtimes").join(PROMOTED);
    fs::create_dir_all(&judged).unwrap();
    fs::create_dir_all(state.join("runtimes/1111111111111111111111111111111111111111")).unwrap();
    symlink("runtimes/1111111111111111111111111111111111111111", state.join("current")).unwrap();
    for program in ["north", "verify-north"] {
        north_base::foundation::shim::write_executable(&judged.join(program), "#!/bin/sh\necho \"wr.md: target whiterabbit claude:$(basename \"$(readlink \"$NORTH_RUNTIME_LINK\")\")\"\n");
    }
    let plan = probes::core_loop("whiterabbit", PROMOTED, &["claude".to_owned()], false, true, None);
    let workers: Vec<&Probe> = plan.probes.iter().filter(|probe| probe.name.contains("verify-north") || probe.name.contains("first turn")).collect();
    assert_eq!(workers.len(), 2, "{:?}", plan.probes.iter().map(|probe| &probe.name).collect::<Vec<_>>());
    for probe in workers {
        let output = Command::new(bash_program()).arg("-c").arg(&probe.script).env("HOME", home.path()).env_remove("XDG_STATE_HOME").env_remove("NORTH_RUNTIME_LINK").output().unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).trim() == format!("wr.md: target whiterabbit claude:{PROMOTED}"), "{}: the session hosts it starts would follow {:?}, not the judged runtime; {}", probe.name, String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert!(output.status.success(), "{}: {}", probe.name, String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
fn the_verify_north_probe_reads_all_of_the_placement_listing_before_judging_it() {
    let home = tempfile::tempdir().unwrap();
    let judged = home.path().join(".local/state/north/runtimes").join(PROMOTED);
    fs::create_dir_all(&judged).unwrap();
    let north = "#!/bin/sh\nif [ \"$1\" = config ]; then\n  echo 'claude:a available'\n  sleep 1\n  echo 'claude:b available'\n  echo 'wr.md: target whiterabbit claude:a'\nfi\n";
    north_base::foundation::shim::write_executable(&judged.join("north"), north);
    north_base::foundation::shim::write_executable(&judged.join("verify-north"), "#!/bin/sh\nexit 0\n");
    let plan = probes::core_loop("whiterabbit", PROMOTED, &["claude".to_owned()], false, true, None);
    let walk = plan.probes.iter().find(|probe| probe.name.contains("verify-north")).unwrap();
    let output = Command::new(bash_program()).arg("-c").arg(&walk.script).env("HOME", home.path()).env_remove("XDG_STATE_HOME").env_remove("NORTH_RUNTIME_LINK").output().unwrap();
    assert!(output.status.success(), "grep -q closed the listing's pipe at its first line and the writer died on it, exit {:?}: {}", output.status.code(), String::from_utf8_lossy(&output.stderr));
}
