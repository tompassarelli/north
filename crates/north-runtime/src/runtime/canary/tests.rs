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
        Self { _dir: dir, layout }
    }

    fn fleet(&self, failing: Option<(&'static str, Ran)>) -> Fleet<'_> {
        Fleet { layout: &self.layout, failing, probed: Mutex::new(Vec::new()), alarms: Mutex::new(Vec::new()) }
    }

    fn current(&self) -> std::path::PathBuf {
        fs::read_link(self.layout.current()).unwrap()
    }
}

fn exited(code: u8, log: &str) -> Ran {
    Ran::Exited { code, log: log.to_owned() }
}

#[test]
fn a_promotion_that_breaks_a_core_loop_probe_is_rolled_back_with_an_alarm_naming_the_probe() {
    let host = Host::promoted();
    let fleet = host.fleet(Some(("ssh gex45 true", exited(255, "ssh: Could not resolve hostname gex45\n"))));
    assert_eq!(check(&host.layout, "nexus", PROMOTED, &fleet), Verdict::RolledBack);
    assert_eq!(fleet.probed.lock().unwrap().len(), 4);
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
fn a_probe_that_could_not_run_leaves_the_promotion_in_place_and_unjudged() {
    let host = Host::promoted();
    let fleet = host.fleet(Some(("opencode", Ran::Unrun("no verdict within 150s".to_owned()))));
    assert!(matches!(check(&host.layout, "nexus", PROMOTED, &fleet), Verdict::Unrun(why) if why.contains("opencode")));
    assert_eq!(host.current(), layout::link(PROMOTED));
    assert!(fleet.alarms.lock().unwrap().is_empty());
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
    let mut lander = Command::new(&lander).args(["land", "serve", "--app-token"]).arg(&token).stdin(std::process::Stdio::piped()).spawn().unwrap();
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
    let _ = lander.kill();
    let _ = lander.wait();
    assert!(probe.status.success(), "{}", String::from_utf8_lossy(&probe.stderr));
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

