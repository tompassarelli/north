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
    executable(&root.join("bin/claude"), &format!("exec {} --trace {trace} \"$@\"\n", support::fake_claude()));
}

fn reader(root: &Path) {
    let session = root.join(".local/state/north/fleet/reader");
    fs::create_dir_all(&session).unwrap();
    let record = serde_json::json!({"name": "reader", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "role": "lead", "stopped": false});
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
    let windows = serde_json::json!([{"window": "5h", "used_pct": 10.0, "burn_pct_h": 1.0, "resets_at": "2026-10-16T16:00:00Z"}]);
    fs::write(agents.join("usage-gate.json"), serde_json::json!({"claude": {"controls": {"claude:a": {"windows": windows}}}}).to_string()).unwrap();
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
    environment(Command::new(env!("CARGO_BIN_EXE_north")), root, host).args(args).envs(sender.iter().copied()).output().unwrap()
}

pub(crate) fn environment(mut command: Command, root: &Path, host: &str) -> Command {
    command
        .current_dir(root)
        .env("HOME", root)
        .env("XDG_STATE_HOME", root.join(".local/state"))
        .env("AGENTS_ORCHESTRATION", root.join("op/orchestration.toml"))
        .env("AGENTS_OPERATOR_DIR", root.join("op"))
        .env("PLACEMENT_NOW", "2026-10-16T12:00:00Z")
        .env("RESIDENT_CAP_HOST", host)
        .env("CLAUDE_RUNTIME", root.join("bin/claude"))
        .env("FLEET_LAUNCH_FIXTURE", root)
        .env("THREADS_DB", root.join("threads.db"))
        .env("NORTH_HOME", env!("CARGO_MANIFEST_DIR"))
        .env("PLACEMENT_HOSTS", root.join("placement-hosts.json"))
        .env("FAKE_CLAUDE_HOOK_MS", "1500")
        .env("FAKE_CLAUDE_SIGNED_OUT_MARK", root.join("signed-out"))
        .env("FAKE_CLAUDE_STALL_MARK", root.join("stalled"))
        .env("NORTH_FLEET_SESSION", "")
        .env_remove("NORTH_FLEET_TEST")
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
    fixture(root, "fold");
    let retried = launch(root);
    let sent = north(root, &["fleet", "send", "--from", "service:fleet-test", "narrow", "answer this"]);
    let answered = |shown: &str| shown.split_once("] answer this").is_some_and(|(_, after)| after.contains("OK"));
    let mut shown = String::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !answered(&shown) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(200));
        shown = text(&north(root, &["fleet", "show", "narrow"]).stdout);
    }
    north(root, &["fleet", "forget", "narrow"]);

    assert!(retried.status.success(), "{}", text(&retried.stderr));
    assert!(sent.status.success(), "{}", text(&sent.stderr));
    assert!(answered(&shown), "the relaunched session never answered the message: {shown}");
}
