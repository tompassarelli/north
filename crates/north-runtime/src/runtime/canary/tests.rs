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
    offloads_hub: bool,
    settles: bool,
    builds: bool,
    landers: Option<probes::Landers>,
    depbox_host: Option<std::path::PathBuf>,
    services: Option<String>,
    checkout: Option<std::path::PathBuf>,
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
        if let Some(listing) = self.services.as_ref().filter(|_| probe.name.contains("host service")) {
            return match services::judge(self.layout, PROMOTED, listing, &[]) {
                Ok(said) => Ran::Exited { code: 0, log: said },
                Err(failure) => classify(Some(i32::from(failure.code)), failure.message),
            };
        }
        let failing = if probe.script.contains(PREVIOUS) { &self.replaced } else { &self.failing };
        match failing {
            Some((name, Ran::Exited { code, log })) if probe.name.contains(name) => Ran::Exited { code: *code, log: log.clone() },
            Some((name, Ran::Unrun(why))) if probe.name.contains(name) => Ran::Unrun(why.clone()),
            Some((name, Ran::Unexecuted { code, log })) if probe.name.contains(name) => Ran::Unexecuted { code: *code, log: log.clone() },
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

    fn offloads_to_hub(&self, _host: &str) -> bool {
        self.offloads_hub
    }

    fn settles_fleet(&self, _host: &str) -> bool {
        self.settles
    }

    fn takes_land_checks(&self, _host: &str) -> bool {
        self.builds
    }

    fn lander_repositories(&self) -> Option<probes::Landers> {
        self.landers.clone()
    }

    fn lander_take_host(&self, _sha: &str) -> Option<String> {
        self.checkout.as_deref().and_then(|checkout| take_host(&checkout.join(".git"), "live", "candidate").ok().flatten())
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
        Fleet { layout: &self.layout, failing, replaced: None, launchable: super::probes::PROVIDERS.iter().map(|provider| (*provider).to_owned()).collect(), reaches_hub: true, offloads_hub: true, settles: false, builds: false, landers: None, depbox_host: None, services: None, checkout: None, probed: Mutex::new(Vec::new()), alarms: Mutex::new(Vec::new()), pages: Mutex::new(Vec::new()) }
    }

    fn settling(&self, failing: Option<(&'static str, Ran)>) -> Fleet<'_> {
        Fleet { settles: true, ..self.fleet(failing) }
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
fn a_shipped_runtime_that_fails_a_worker_cargo_run_stays_off_the_host() {
    let host = Host::promoted();
    let fleet = host.fleet(Some(("reaches gex45 through", exited(255, "ssh: Could not resolve hostname gex45\n"))));
    assert_eq!(check(&host.layout, "nexus", PROMOTED, &fleet), Verdict::RolledBack);
    assert_eq!(fleet.probed.lock().unwrap().len(), 8);
    assert_eq!(host.current(), layout::link(PREVIOUS));
    assert_eq!(lifecycle::state_of::<Promoting>(&host.layout, PROMOTED).unwrap(), Some(Promotion::RolledBack));
    let alarms = fleet.alarms.lock().unwrap();
    assert_eq!(alarms.len(), 1);
    assert!(alarms[0].contains("probe `a sandboxed worker shell reaches gex45 through north capacity offload` exited 255"), "{}", alarms[0]);
    assert!(alarms[0].contains("Could not resolve hostname gex45"), "{}", alarms[0]);
    assert!(alarms[0].contains("rolled back to 11111111"), "{}", alarms[0]);
    assert!(matches!(update::hold::verdict(&host.layout, PROMOTED, true).unwrap(), update::hold::Verdict::Held(_)));
    assert_eq!(lifecycle::state_of::<Canarying>(&host.layout, &format!("nexus/{PROMOTED}")).unwrap(), Some(Canary::RolledBack));
    assert!(pin::read(&host.layout.state).iter().any(|event| matches!(event, Event::Canaried { sha, host, seconds, .. } if sha == PROMOTED && host == "nexus" && *seconds < 60)), "the canary journals the stage seconds the scoreboard reads");

    let host = Host::promoted();
    let fleet = Fleet { builds: true, ..host.fleet(Some(("an offloaded land check placed on this host", exited(1, "the offloaded land check on cpx62 returned no verdict (exit 75)\n")))) };
    assert_eq!(check(&host.layout, "cpx62", PROMOTED, &fleet), Verdict::RolledBack, "on 2026-11-13 order 45 passed cpx62's three probes and no offloaded land check there returned a verdict for an hour");
    assert_eq!(host.current(), layout::link(PREVIOUS));
    let alarms = fleet.alarms.lock().unwrap();
    assert!(alarms[0].contains("probe `an offloaded land check placed on this host returns a verdict` exited 1"), "{}", alarms[0]);

    let host = Host::promoted();
    let failing = Fleet { builds: true, ..host.fleet(Some(("a worker cargo run", exited(1, "capacity gate: cannot run cargo\n")))) };
    let refused = preflight(&host.layout, "gex45", PROMOTED, Some(PREVIOUS), &failing).unwrap_err();
    assert!(refused.contains("probe `a worker cargo run through the capacity gate runs on this host` exited 1"), "on 2026-11-15 runtimes holding aea18d3b3e went live on gex45 and cpx62b and failed every worker cargo run: {refused}");

    let deferred = Fleet { builds: true, ..host.fleet(Some(("a worker cargo run", Ran::Unrun("admission deferred the run".to_owned())))) };
    assert_eq!(preflight(&host.layout, "cpx62b", PROMOTED, Some(PREVIOUS), &deferred), Ok(()), "a run admission defers is not run and holds no repair off the host");

    let unexecuted = Fleet { builds: true, ..host.fleet(Some(("a worker cargo run", classify(Some(127), "north: command not found\n".to_owned())))) };
    assert!(preflight(&host.layout, "gex45", PROMOTED, Some(PREVIOUS), &unexecuted).unwrap_err().contains("exited 127"), "a worker cargo run the judged runtime could not execute was released as not run");
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

/// The fleet registry's previous schema, as `crates/north-fleet-store/src/store/tests.rs` builds it: the events
/// kinds before the fleet's own, and a fleet_sessions table before the display columns.
const PREVIOUS_FLEET: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL CHECK(kind IN ('claim','release','KICKOFF','ANSWER')), actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;
CREATE INDEX events_item ON events(item, seq);
CREATE TABLE fleet_sessions(name TEXT PRIMARY KEY, host TEXT NOT NULL, status TEXT NOT NULL, created TEXT NOT NULL, updated TEXT NOT NULL) STRICT;";

#[test]
fn the_launch_probes_scratch_root_starts_at_the_previous_schema_version() {
    let previous = tempfile::tempdir().unwrap();
    let source = previous.path().join("threads.db");
    let live = rusqlite::Connection::open(&source).unwrap();
    live.execute_batch(PREVIOUS_FLEET).unwrap();
    live.execute("INSERT INTO events(ts, kind, data) VALUES ('2026-01-01T00:00:00Z', 'claim', '{}')", []).unwrap();
    live.execute("INSERT INTO fleet_sessions(name, host, status, created, updated) VALUES ('old', 'here', 'starting', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')", []).unwrap();
    drop(live);
    let scratch = tempfile::tempdir().unwrap();

    let (state, _) = launch::scratch_root_from(scratch.path(), Some(&source)).unwrap();

    let ledger = rusqlite::Connection::open(state.join("threads.db")).unwrap();
    let events: String = ledger.query_row("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'events'", [], |row| row.get(0)).unwrap();
    assert!(!events.contains("'MESSAGE'") && !events.contains("'SPEND'"), "the scratch ledger starts at the previous fleet schema, not the current one: {events}");
    let rows: i64 = ledger.query_row("SELECT count(*) FROM events WHERE kind = 'claim' AND ts = '2026-01-01T00:00:00Z'", [], |row| row.get(0)).unwrap();
    assert_eq!(rows, 1, "a previous-schema row is kept for the widened table");
    let sessions: String = ledger.query_row("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'fleet_sessions'", [], |row| row.get(0)).unwrap();
    assert!(!sessions.contains("provider"), "the scratch fleet table starts before its display columns: {sessions}");
}

#[test]
fn a_probe_answer_is_judged_by_the_policy_it_was_given_not_by_the_wording_of_a_heading() {
    let token = "north-canary-0123456789abcdef".to_owned();
    let planted = launch::Evidence::Token(token.clone());
    assert_eq!(launch::carries(&format!("Tom\u{2019}s policy\n{token}"), &planted), Ok(()), "455c0e5c: a typographic apostrophe");
    assert_eq!(launch::carries(&format!("Act and finish {}  ", token.to_uppercase()), &planted), Ok(()), "12f6d9b6: the first section heading");
    assert!(launch::carries("Act and finish", &planted).is_err(), "a worker that never saw the policy has no token");
    assert_eq!(launch::carries("ok", &launch::Evidence::Nothing), Ok(()));
    let policy = "# Tom's policy\n\n## Hard boundaries\n\n- Keep secrets out of output, logs and commits; use the repository's encrypted mechanism.\n- Never force-push or rewrite history that is already pushed.\n\n## Code\n\n- Never weaken tests or retry flakes to green.\n- Add a scenario only for something you can point to; \"defensive\" is not evidence.\n";
    let scratch = tempfile::tempdir().unwrap();
    let (planted, agents) = launch::evidence("pi", Some(policy.to_owned()), scratch.path()).unwrap();
    let launch::Evidence::Token(token) = &planted else { panic!("pi gets its policy from deliver, so its probe plants a token: {planted:?}") };
    let delivered = fs::read_to_string(agents.unwrap().join("current/instructions/shared/AGENTS.md")).unwrap();
    assert!(delivered.contains(token.as_str()) && delivered.contains(policy.trim()), "{delivered}");
    assert!(launch::carries("Evidence", &planted).is_err(), "df343034 on the laptop: a heading of the worker instructions is no proof");
    let (asked, agents) = launch::evidence("claude", Some(policy.to_owned()), scratch.path()).unwrap();
    assert_eq!(asked, launch::Evidence::Sentence { opening: "Never weaken tests".to_owned(), whole: vec!["Never weaken tests or retry flakes to green.".to_owned()] });
    assert!(agents.is_none(), "a Claude probe plants nothing, because its policy comes from the live profile");
    assert_eq!(launch::carries("Never weaken tests or retry flakes to green", &asked), Ok(()));
    assert!(launch::carries("Evidence", &asked).is_err());
    assert!(launch::carries("Never force-push or rewrite history that is already pushed.", &asked).is_err(), "the wrong sentence of the policy still fails");
    let republished = policy.replace("retry flakes to green", "retry a flake until it passes");
    assert!(launch::carries("Never weaken tests or retry a flake until it passes.", &asked).is_err());
    assert_eq!(launch::carries("Never weaken tests and retry flakes until they go green.", &asked), Ok(()), "14 Nov canary rollback of e2f6224c4e: the same key words in other wording are no runtime fault");
    assert!(launch::carries("I was given no policy text that begins \"Never weaken tests\".", &asked).is_err(), "a worker whose policy file is withheld quotes nothing of it");
    assert_eq!(launch::carries("Never weaken tests or retry a flake until it passes.", &asked.or_in(Some(&republished))), Ok(()), "a publish during the probe");
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
fn a_canary_that_cannot_place_its_probe_is_not_judged() {
    let home = tempfile::tempdir().unwrap();
    let judged = home.path().join(".local/state/north/runtimes").join(PROMOTED);
    fs::create_dir_all(&judged).unwrap();
    let ran = home.path().join("ran");
    north_base::foundation::shim::write_executable(&judged.join("verify-north"), format!("#!/bin/sh\necho \"$@\" > {}\n", ran.display()));
    let plan = probes::core_loop("whiterabbit", PROMOTED, &["claude".to_owned()], false, false, true, None);
    let walk = plan.probes.iter().find(|probe| probe.name.contains("verify-north")).unwrap();
    let run = |listing: &str| {
        let (first, rest) = listing.split_once('\n').unwrap();
        north_base::foundation::shim::write_executable(&judged.join("north"), format!("#!/bin/sh\ncat <<'EOF'\n{first}\nEOF\nsleep 1\ncat <<'EOF'\n{rest}\nEOF\n"));
        Command::new(bash_program()).arg("-c").arg(&walk.script).env("HOME", home.path()).env_remove("XDG_STATE_HOME").env_remove("NORTH_RUNTIME_LINK").output().unwrap()
    };
    let unread = run("  claude:a available: 5h unread, week 40% (60% headroom)\nwr.md: target whiterabbit claude:a");
    assert_eq!(unread.status.code(), Some(75), "the probe placed its worker on an account whose 5h window reads unread: {}", String::from_utf8_lossy(&unread.stderr));
    assert!(!ran.exists());
    let other = run("  claude:a available: 5h unread, week 40% (60% headroom)\n  claude:c available: 5h 7%, week 1% (93% headroom)\nwr.md: target whiterabbit claude:a");
    assert!(other.status.success(), "{}", String::from_utf8_lossy(&other.stderr));
    assert!(fs::read_to_string(&ran).unwrap().contains("--claude-account c"));
}

fn runtime_history(changed: &str, hosts: &str) -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| assert!(Command::new("git").arg("-C").arg(repo.path()).args(["-c", "user.name=canary", "-c", "user.email=canary@example.invalid", "-c", "commit.gpgsign=false"]).args(args).status().unwrap().success());
    git(&["init", "-q", "-b", "main"]);
    fs::write(repo.path().join(".safe-push"), format!("[landing]\n\tremote-hosts = {hosts}\n")).unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "live"]);
    git(&["tag", "live"]);
    let path = repo.path().join(changed);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "changed\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "candidate"]);
    git(&["tag", "candidate"]);
    repo
}

#[test]
fn a_runtime_whose_lander_refuses_a_build_host_lane_is_rolled_back() {
    let refused = || ("throwaway lane", exited(1, "the promoted lander decided cpx62's throwaway lane 'conflict unreadable'\n"));
    let history = runtime_history("crates/north-landing/src/land/candidate.rs", "cpx62");
    let host = Host::promoted();
    let fleet = Fleet { checkout: Some(history.path().to_path_buf()), ..host.settling(Some(refused())) };
    assert_eq!(check(&host.layout, "whiterabbit", PROMOTED, &fleet), Verdict::RolledBack, "on 2026-11-14 a lander change had every build-host hand-in decided 'conflict unreadable' and went live");
    assert_eq!(host.current(), layout::link(PREVIOUS));
    let alarms = fleet.alarms.lock().unwrap();
    assert!(alarms[0].contains("a build host's throwaway lane is judged by the promoted lander") && alarms[0].contains("conflict unreadable"), "{}", alarms[0]);

    let history = runtime_history("docs/rationale/landing.md", "cpx62");
    let host = Host::promoted();
    let fleet = Fleet { checkout: Some(history.path().to_path_buf()), ..host.settling(Some(refused())) };
    assert_eq!(check(&host.layout, "whiterabbit", PROMOTED, &fleet), Verdict::Passed);
    assert!(!fleet.probed.lock().unwrap().iter().any(|name| name.contains("throwaway lane")), "a promotion that changes no lander or take path runs the probe");

    for hosts in ["$(touch${IFS}owned)", "-oProxyCommand=touch", "cpx62&touch"] {
        let history = runtime_history("crates/north-lander/src/land/serve.rs", hosts);
        assert_eq!(take_host(&history.path().join(".git"), "live", "candidate"), Ok(None), "{hosts}");
    }
    let history = runtime_history("crates/north-lander/src/land/serve.rs", "cpx62");
    fs::write(history.path().join(".safe-push"), "[landing]\n\tremote-hosts = $(touch owned)\n").unwrap();
    assert_eq!(take_host(&history.path().join(".git"), "live", "candidate"), Ok(Some("cpx62".to_owned())), "the host is read from the candidate commit, not the working tree");
}

#[test]
fn a_promotion_judge_fetches_a_candidate_its_mirror_lacks() {
    let origin = runtime_history("crates/north-landing/src/land/candidate.rs", "cpx62");
    let sha = |name: &str| String::from_utf8(Command::new("git").arg("-C").arg(origin.path()).args(["rev-parse", name]).output().unwrap().stdout).unwrap().trim().to_owned();
    let mirror = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| assert!(Command::new("git").arg("--git-dir").arg(mirror.path()).args(args).status().unwrap().success());
    git(&["init", "-q", "--bare"]);
    git(&["config", "remote.origin.url", &origin.path().to_string_lossy()]);
    git(&["config", "remote.origin.fetch", "+refs/heads/main:refs/remotes/origin/main"]);
    git(&["fetch", "-q", "origin", "live:refs/remotes/origin/main"]);
    assert_eq!(take_host(mirror.path(), &sha("live"), &sha("candidate")), Ok(Some("cpx62".to_owned())), "a mirror that had not yet fetched the candidate skipped the build host probe silently");
    assert!(take_host(mirror.path(), &sha("live"), PROMOTED).is_err(), "a candidate origin lacks too is reported, not skipped silently");
}
