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
    launchable: Vec<String>,
    reaches_hub: bool,
    settles: bool,
    landers: Option<probes::Landers>,
    probed: Mutex<Vec<String>>,
    alarms: Mutex<Vec<String>>,
}

impl Machine for Fleet<'_> {
    fn probe(&self, probe: &Probe) -> Ran {
        self.probed.lock().unwrap().push(probe.name.clone());
        match &self.failing {
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
        Fleet { layout: &self.layout, failing, launchable: super::probes::PROVIDERS.iter().map(|provider| (*provider).to_owned()).collect(), reaches_hub: true, settles: false, landers: None, probed: Mutex::new(Vec::new()), alarms: Mutex::new(Vec::new()) }
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
    let fleet = host.fleet(Some(("ssh gex45 true", exited(255, "ssh: Could not resolve hostname gex45\n"))));
    assert_eq!(check(&host.layout, "nexus", PROMOTED, &fleet), Verdict::RolledBack);
    assert_eq!(fleet.probed.lock().unwrap().len(), 5);
    assert_eq!(host.current(), layout::link(PREVIOUS));
    assert_eq!(lifecycle::state_of::<Promoting>(&host.layout, PROMOTED).unwrap(), Some(Promotion::RolledBack));
    let alarms = fleet.alarms.lock().unwrap();
    assert_eq!(alarms.len(), 1);
    assert!(alarms[0].contains("probe `a sandboxed worker shell runs ssh gex45 true` exited 255"), "{}", alarms[0]);
    assert!(alarms[0].contains("Could not resolve hostname gex45"), "{}", alarms[0]);
    assert!(alarms[0].contains("rolled back to 11111111"), "{}", alarms[0]);
    assert!(matches!(update::hold::verdict(&host.layout, PROMOTED, true).unwrap(), update::hold::Verdict::Held(_)));
    assert_eq!(lifecycle::state_of::<Canarying>(&host.layout, &format!("nexus/{PROMOTED}")).unwrap(), Some(Canary::RolledBack));
}

#[test]
fn the_hub_reached_as_user_at_host_probes_its_fetch_and_never_ssh_to_itself() {
    let host = Host::promoted();
    let fleet = host.fleet(None);
    assert_eq!(check(&host.layout, "tom@gex45", PROMOTED, &fleet), Verdict::Passed);
    let probed = fleet.probed.lock().unwrap();
    assert!(probed.iter().all(|name| !name.contains("ssh gex45")), "{probed:?}");
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
fn the_rollback_target_is_trusted_and_never_judged_again() {
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
    assert!(probed.iter().all(|name| !name.contains("ssh gex45")), "{probed:?}");
    assert!(probed.iter().any(|name| name.contains("claude")), "{probed:?}");
}

#[test]
fn a_launch_probe_fails_when_the_worker_cannot_quote_the_shared_policy_heading() {
    assert_eq!(launch::carries("# Tom's policy", Some("Tom's policy")), Ok(()));
    assert_eq!(launch::carries("ok", Some("Tom's policy")), Err("Tom's policy"));
    assert_eq!(launch::carries("ok", None), Ok(()));
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
