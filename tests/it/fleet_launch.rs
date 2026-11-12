use crate::support;

use north_core::foundation::shim;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const BRIEF: &str = "Item: makgora#4 LAN check\nSurface: lua\nNeeds: warcraft\n";

fn executable(path: &Path, body: &str) {
    shim::write_script(path, body);
}

static FIXTURE: std::sync::LazyLock<support::template::Template> = std::sync::LazyLock::new(|| support::template::Template::build(shared_fixture));

pub(crate) fn fixture(root: &Path, trace: &str) {
    FIXTURE.copy_into(root);
    reader(root);
    provider(root, trace);
}

fn provider(root: &Path, trace: &str) {
    executable(&root.join("bin/claude"), &format!("exec {} --trace {trace} \"$@\"\n", crate::support::bin_exe!("CARGO_BIN_EXE_fake_claude")));
}

fn reader(root: &Path) {
    let session = root.join(".local/state/north/fleet/reader");
    fs::create_dir_all(&session).unwrap();
    let record = serde_json::json!({"name": "reader", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "role": "lead"});
    fs::write(session.join("session.json"), record.to_string()).unwrap();
}

fn shared_fixture(root: &Path) {
    let op = root.join("op");
    let agents = root.join(".local/state/agents");
    let profile = root.join(".local/state/north/profiles/claude-a");
    for directory in [&op, &agents, &profile, &root.join("bin"), &root.join("units")] {
        fs::create_dir_all(directory).unwrap();
    }
    fs::write(op.join("orchestration.toml"), "[placement]\nprivate_repos = []\n[hosts.here]\nserves = [\"warcraft\"]\n[tiers.\"claude:haiku\"]\nmodel = \"claude-haiku-5-5\"\neffort = \"high\"\n[tiers.\"claude:sonnet-high\"]\nmodel = \"claude-sonnet-5-5\"\neffort = \"high\"\n[tiers.\"claude:opus-medium\"]\nmodel = \"claude-opus-5-5\"\neffort = \"medium\"\n").unwrap();
    fs::write(profile.join(".credentials.json"), "{}").unwrap();
    fs::write(profile.join("settings.json"), r#"{"hooks": {"PreToolUse": []}}"#).unwrap();
    let read_at = north_core::foundation::clock::Timestamp::now().iso();
    let windows = serde_json::json!([{"window": "5h", "used_pct": 10.0, "resets_at": "2099-01-01T00:00:00Z", "as_of": read_at}, {"window": "week", "used_pct": 10.0, "resets_at": "2099-01-01T00:00:00Z", "as_of": read_at}]);
    fs::write(agents.join("usage-gate.json"), serde_json::json!({"claude": {"windows": {"claude:a": windows}}}).to_string()).unwrap();
    let setsid = Command::new("sh").args(["-c", "command -v setsid"]).output().unwrap();
    executable(&root.join("bin/setsid"), &format!("exec {} \"$@\"\n", String::from_utf8_lossy(&setsid.stdout).trim()));
    executable(
        &root.join("bin/systemd-run"),
        "case \" $* \" in *' --scope '*) ;; *)\n  for a; do case $a in --unit=*) unit=${a#--unit=};; --) break;; esac; done\n  while [ \"$1\" != -- ]; do case $1 in --setenv=*) export \"${1#--setenv=}\";; esac; shift; done\n  shift\n  setsid \"$@\" < /dev/null > /dev/null 2>&1 &\n  echo $! > \"$FLEET_LAUNCH_FIXTURE/units/$unit\"\n  exit 0;;\nesac\nwhile [ \"$1\" != -- ]; do [ \"$1\" = --unit ] && unit=$2; shift; done\nshift\necho $$ > \"$FLEET_LAUNCH_FIXTURE/units/$unit\"\nexec \"$@\"\n",
    );
    executable(
        &root.join("bin/systemctl"),
        "for a; do last=$a; done\nunit=\"$FLEET_LAUNCH_FIXTURE/units/$last\"\ncase \" $* \" in\n  *' --user stop '*)\n    [ -f \"$unit\" ] || { echo \"Failed to stop $last: Unit $last not loaded.\" >&2; exit 5; }\n    kill -s TERM -- \"-$(cat \"$unit\")\" 2>/dev/null\n    rm -f \"$unit\";;\n  *' show '*) pid=$(cat \"$unit\" 2>/dev/null); if [ -n \"$pid\" ] && kill -0 \"$pid\" 2>/dev/null; then echo \"$pid\"; else echo 0; fi;;\nesac\n",
    );
    let ledger = north(root, &["threads", "release", "north#1", "--by", "setup"]);
    assert!(String::from_utf8_lossy(&ledger.stderr).contains("has no owner"), "{}", String::from_utf8_lossy(&ledger.stderr));
}

fn north(root: &Path, args: &[&str]) -> Output {
    north_on(root, "here", args)
}

fn north_on(root: &Path, host: &str, args: &[&str]) -> Output {
    north_as(root, host, &[], args)
}

fn north_as(root: &Path, host: &str, sender: &[(&str, &str)], args: &[&str]) -> Output {
    environment(Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north")), root, host).args(args).envs(sender.iter().copied()).output().unwrap()
}

pub(crate) fn environment(mut command: Command, root: &Path, host: &str) -> Command {
    command
        .current_dir(root)
        .env("HOME", root)
        .env("XDG_STATE_HOME", root.join(".local/state"))
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("NORTH_TEST_ROOT", root)
        .env("AGENTS_ORCHESTRATION", root.join("op/orchestration.toml"))
        .env("AGENTS_OPERATOR_DIR", root.join("op"))
        .env("PLACEMENT_NOW", "2026-10-16T12:00:00Z")
        .env("RESIDENT_CAP_HOST", host)
        .env("CLAUDE_RUNTIME", root.join("bin/claude"))
        .env("FLEET_LAUNCH_FIXTURE", root)
        .env("THREADS_DB", root.join("threads.db"))
        .env("NORTH_HOME", crate::support::manifest_dir())
        .env("PLACEMENT_HOSTS", root.join("placement-hosts.json"))
        .env("FAKE_CLAUDE_HOOK_MS", "1500")
        .env("FAKE_CLAUDE_SIGNED_OUT_MARK", root.join("signed-out"))
        .env("FAKE_CLAUDE_STALL_MARK", root.join("stalled"))
        .env("NORTH_FLEET_SESSION", "")
        .env_remove("NORTH_FLEET_TEST")
        .env_remove("NIRI_SOCKET")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()));
    command
}

fn launch(root: &Path) -> Output {
    launch_on(root, "here")
}

fn launch_on(root: &Path, host: &str) -> Output {
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    let brief = brief.display().to_string();
    north_on(root, host, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--system-prompt", &brief, "start the box"])
}

/// A verification launch skips placement and the spend check, so a first turn can be fed on an account whatever
/// its sampled windows read.
fn launch_test(root: &Path) -> Output {
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    let brief = brief.display().to_string();
    north(root, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--test", "--system-prompt", &brief, "start the box"])
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_launch_whose_provider_exits_before_the_host_registers_leaves_no_session_directory_and_the_same_name_launches_and_answers() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    executable(&root.join("bin/claude"), "exit 1\n");
    let session = root.join(".local/state/north/fleet/narrow");

    let failed = launch(root);

    assert!(!failed.status.success(), "{}", text(&failed.stdout));
    assert!(!session.exists(), "the failed launch left {session:?}: {:?}", fs::read_dir(&session).map(|entries| entries.flatten().map(|entry| entry.file_name()).collect::<Vec<_>>()));
    provider(root, "fold");
    let retried = launch(root);
    let sent = north(root, &["fleet", "send", "--from", "service:fleet-test", "narrow", "answer this"]);
    let answered = |shown: &str| shown.split_once("] answer this").is_some_and(|(_, after)| after.contains("OK"));
    let mut shown = String::new();
    north_core::platform::wait::until(std::time::Duration::from_secs(20), || {
        shown = text(&north(root, &["fleet", "show", "narrow"]).stdout);
        answered(&shown)
    });
    north(root, &["fleet", "forget", "narrow"]);

    assert!(retried.status.success(), "{}", text(&retried.stderr));
    assert!(sent.status.success(), "{}", text(&sent.stderr));
    assert!(answered(&shown), "the relaunched session never answered the message: {shown}");
}

#[test]
fn a_120_character_session_name_launches_and_answers_its_first_turn() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "idle-interrupt");
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    let brief = brief.display().to_string();
    let name: String = format!("north-1042-d0432-{}", "the-machine-refusal-detector-reads-the-refused-machine-".repeat(3)).chars().take(120).collect();
    assert_eq!(name.len(), 120);

    let launched = north(root, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", &name, "--parent", "none", "--system-prompt", &brief, "start the box"]);
    let stderr = text(&launched.stderr);
    let mut shown = String::new();
    let answered = north_core::platform::wait::until(std::time::Duration::from_secs(20), || {
        shown = text(&north(root, &["fleet", "show", &name]).stdout);
        shown.contains("OK")
    });
    north(root, &["fleet", "forget", &name]);

    assert!(launched.status.success(), "{stderr}");
    assert!(answered, "the long-named session never answered its first turn: {shown}");
}

#[test]
fn a_launch_starts_while_the_ledger_host_is_too_loaded_to_say_who_holds_its_task() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    fs::create_dir_all(root.join(".config/threads")).unwrap();
    fs::write(root.join(".config/threads/remote"), "loaded-ledger-host\n").unwrap();
    fs::create_dir_all(root.join(".local/state/threads")).unwrap();
    fs::copy(root.join("threads.db"), root.join(".local/state/threads/snapshot.db")).unwrap();
    executable(&root.join("bin/north"), &format!("exec {} \"$@\"\n", crate::support::bin_exe!("CARGO_BIN_EXE_north")));
    executable(&root.join("bin/ssh"), &format!("for word; do command=$word; done\ncase $command in *holder*) sleep 3;; esac\nTHREADS_DB={} exec sh -c \"$command\"\n", root.join("threads.db").display()));
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    let brief = brief.display().to_string();
    let far_ledger = |args: &[&str]| environment(Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north")), root, "here").env_remove("THREADS_DB").args(args).output().unwrap();

    let launched = far_ledger(&["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--test", "--task", "makgora#4", "--system-prompt", &brief, "start the box"]);
    let listed = text(&north(root, &["fleet", "list"]).stdout);
    north(root, &["fleet", "forget", "narrow"]);

    assert!(launched.status.success(), "{}", text(&launched.stderr));
    assert!(text(&launched.stderr).contains("narrow launches with its holder check left to its claim, because the ledger did not say who holds makgora#4"), "{}", text(&launched.stderr));
    assert!(listed.contains("narrow"), "{listed}");
}

#[test]
fn a_test_launch_on_a_two_rung_route_runs_its_first_rung_and_carries_the_end_your_turn_note() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");

    let launched = north(root, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--test", "--route", "haiku:medium,opus:medium", "start the box"]);
    let session = fs::read_to_string(root.join(".local/state/north/fleet/narrow/session.json"));
    north(root, &["fleet", "forget", "narrow"]);

    assert!(launched.status.success(), "{}", text(&launched.stderr));
    let session: serde_json::Value = serde_json::from_str(&session.unwrap()).unwrap();
    assert_eq!(session["model"], "claude-haiku-5-5", "{session}");
    assert_eq!(session["effort"], "medium", "{session}");
    assert!(session["first_attempt"].as_str().is_some_and(|brief| brief.contains("do not retry: end your turn at once")), "{session}");
}

#[test]
fn a_second_rung_launch_the_route_refuses_is_told_to_the_parent_that_started_the_first_rung() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let orchestration = root.join("op/orchestration.toml");
    let tiers = fs::read_to_string(&orchestration).unwrap();
    fs::write(&orchestration, tiers.replace("[tiers.\"claude:opus-medium\"]\nmodel = \"claude-opus-5-5\"\neffort = \"medium\"\n", "")).unwrap();
    north(root, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "lead", "--parent", "none", "--test", "start the lead"]);

    let refused = north(root, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow-r2", "--parent", "lead", "--test", "--route", "after:narrow:opus", "start the box"]);
    let mut shown = String::new();
    north_core::platform::wait::until(std::time::Duration::from_secs(20), || {
        shown = text(&north(root, &["fleet", "show", "lead"]).stdout);
        shown.contains("the second rung did not start")
    });
    north(root, &["fleet", "forget", "lead"]);

    assert!(!refused.status.success(), "{}", text(&refused.stdout));
    assert!(shown.contains("[from narrow-r2] route: the second rung did not start"), "the refused second rung never reached its parent: {shown}");
}

#[test]
fn a_test_launch_without_a_declared_fixture_root_refuses_before_it_writes_a_session() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    let brief = brief.display().to_string();

    let refused = environment(Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north")), root, "here")
        .env_remove("NORTH_TEST_ROOT")
        .args(["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--test", "--system-prompt", &brief, "start the box"])
        .output()
        .unwrap();

    assert!(!refused.status.success(), "{}", text(&refused.stdout));
    assert!(text(&refused.stderr).contains("NORTH_TEST_ROOT"), "{}", text(&refused.stderr));
    assert!(!root.join(".local/state/north/fleet/narrow").exists());

    let orchestration = root.join("op/orchestration.toml");
    let mut hosts = fs::read_to_string(&orchestration).unwrap();
    hosts.push_str("[hosts.gex45]\nssh = \"gex45\"\n");
    fs::write(&orchestration, hosts).unwrap();
    executable(&root.join("bin/ssh"), "exit 1\n");
    let brief = root.join(".local/state/agents/handoffs/proxy-ds/b1020loop.md");
    fs::create_dir_all(brief.parent().unwrap()).unwrap();
    fs::write(&brief, "the box\n").unwrap();
    let prompt = format!("read {} before you start", brief.display());
    let placed = north(root, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--test", "--cwd", "gex45:/tmp", &prompt]);
    let stderr = text(&placed.stderr);

    assert!(!placed.status.success(), "{}", text(&placed.stdout));
    assert!(stderr.contains(&brief.display().to_string()), "{stderr}");
    assert!(stderr.contains("under this host's home, which gex45 cannot read"), "{stderr}");
    assert!(!root.join(".local/state/north/fleet/narrow").exists(), "{stderr}");
}

/// The sampled windows the placement gate reads, with the account's week at `week` percent.
fn usage_gate(root: &Path, week: f64) {
    let read_at = north_core::foundation::clock::Timestamp::now().iso();
    let windows = serde_json::json!([
        {"window": "5h", "used_pct": 10.0, "resets_at": "2099-01-01T00:00:00Z", "as_of": read_at},
        {"window": "week", "used_pct": week, "resets_at": "2099-01-01T00:00:00Z", "as_of": read_at},
    ]);
    fs::write(root.join(".local/state/agents/usage-gate.json"), serde_json::json!({"claude": {"windows": {"claude:a": windows}}}).to_string()).unwrap();
}

#[test]
fn a_first_turn_at_the_weekly_limit_marks_the_account_refused_and_north_accounts_shows_it_unavailable() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "weekly-limit");
    // On 2026-11-10 north read claude:c at 99% of its week and kept it for canaries; the provider then refused its first turn.
    usage_gate(root, 99.0);

    let refused = launch_test(root);

    let mark = root.join(".local/state/north/account-refused/a");
    let written = fs::read_to_string(&mark).unwrap_or_else(|error| panic!("no refusal mark at {}: {error}\n{}", mark.display(), text(&refused.stderr)));
    let text_written = "You've hit your weekly limit · resets Nov 12, 7pm";
    assert!(written.contains(text_written), "{written}");
    assert!(text(&refused.stderr).contains(text_written), "{}", text(&refused.stderr));

    let accounts = north(root, &["accounts"]);
    let listed = text(&accounts.stdout);
    assert!(accounts.status.success(), "{listed}{}", text(&accounts.stderr));
    let row = listed.lines().find(|line| line.starts_with("a ")).unwrap_or_else(|| panic!("no row for a: {listed}"));
    assert!(row.contains(" held ") && row.contains("refused by its client: You've hit"), "{listed}");
    assert!(!row.contains(" open "), "a 99% reading was offered as headroom after the provider refused it: {listed}");
}

#[test]
fn a_revived_proxy_opens_in_a_window_on_the_desktop_and_starts_no_host_where_there_is_none() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "idle-interrupt");
    let profile = root.join(".local/state/north/profiles/claude-a");
    fs::write(profile.join("settings.json"), r#"{"hooks": {"PreToolUse": []}, "skipDangerousModePermissionPrompt": true}"#).unwrap();
    fs::write(profile.join(".claude.json"), serde_json::json!({"projects": {root.display().to_string(): {"hasTrustDialogAccepted": true}}}).to_string()).unwrap();
    let session = root.join(".local/state/north/fleet/proxy");
    fs::create_dir_all(&session).unwrap();
    let record = serde_json::json!({"name": "proxy", "provider": "claude", "account": "a", "cwd": root, "model": null, "created_ms": 1, "host_pid": null, "conversation": "6d1f6f3c-1048-4c1e-9a52-6a0c8b0e1048", "host_dir": session, "role": "proxy", "stopped": false});
    fs::write(session.join("session.json"), record.to_string()).unwrap();
    let spawned = root.join("niri-spawned");
    executable(&root.join("bin/niri"), &format!("case \"$*\" in\n  *'action spawn'*) printf '%s\\n' \"$*\" >> {0}; exit 0;;\n  *'-j windows'*) if [ -f {0} ]; then echo '[{{\"id\":7,\"pid\":1,\"app_id\":\"com.mitchellh.ghostty\",\"workspace_id\":1,\"layout\":{{\"pos_in_scrolling_layout\":[1,1]}}}}]'; else echo '[]'; fi;;\nesac\n", spawned.display()));

    let headless = north(root, &["fleet", "revive", "proxy"]);
    let host_after_refusal = serde_json::from_str::<serde_json::Value>(&fs::read_to_string(session.join("session.json")).unwrap()).unwrap()["host_pid"].clone();
    fs::create_dir_all(root.join("run")).unwrap();
    fs::write(root.join("run/niri.wayland-1.1.sock"), "").unwrap();
    let shown = north(root, &["fleet", "revive", "proxy"]);
    let opened = fs::read_to_string(&spawned).unwrap_or_default();
    north(root, &["fleet", "forget", "proxy"]);

    assert!(!headless.status.success(), "{}", text(&headless.stdout));
    assert!(text(&headless.stderr).contains("runs only in a window Tom types into"), "{}", text(&headless.stderr));
    assert!(host_after_refusal.is_null(), "a refused revive started host {host_after_refusal}");
    assert!(shown.status.success(), "{}", text(&shown.stderr));
    assert!(opened.contains("--title=north proxy -e env") && opened.ends_with("fleet interactive proxy --after-turn\n"), "{opened}");
}

