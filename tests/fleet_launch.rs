mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const BRIEF: &str = "Item: smashcraft#4 LAN check\nSurface: lua\nNeeds: warcraft\n";

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn fixture(root: &Path, trace: &str) {
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
    executable(&root.join("bin/setsid"), &format!("#!/bin/sh\nexec {} \"$@\"\n", String::from_utf8_lossy(&setsid.stdout).trim()));
    executable(&root.join("bin/systemd-run"), "#!/bin/sh\nwhile [ \"$1\" != -- ]; do [ \"$1\" = --unit ] && unit=$2; shift; done\nshift\necho $$ > \"$FLEET_LAUNCH_FIXTURE/units/$unit\"\nexec \"$@\"\n");
    executable(&root.join("bin/systemctl"), "#!/bin/sh\n[ \"$1 $2\" = '--user stop' ] || exit 0\nunit=\"$FLEET_LAUNCH_FIXTURE/units/$3\"\n[ -f \"$unit\" ] || { echo \"Failed to stop $3: Unit $3 not loaded.\" >&2; exit 5; }\nkill -s TERM -- \"-$(cat \"$unit\")\" 2>/dev/null\nrm -f \"$unit\"\n");
    executable(&root.join("bin/claude"), &format!("#!/bin/sh\nexec {} --trace {trace} \"$@\"\n", support::fake_claude()));
}

fn north(root: &Path, args: &[&str]) -> Output {
    north_on(root, "here", args)
}

fn north_on(root: &Path, host: &str, args: &[&str]) -> Output {
    north_as(root, host, &[], args)
}

fn north_as(root: &Path, host: &str, sender: &[(&str, &str)], args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_north"))
        .args(args)
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
        .env("PLACEMENT_HOSTS", root.join("placement-hosts.json"))
        .env("FAKE_CLAUDE_HOOK_MS", "1500")
        .env("FAKE_CLAUDE_SIGNED_OUT_MARK", root.join("signed-out"))
        .env("FAKE_CLAUDE_STALL_MARK", root.join("stalled"))
        .env("NORTH_FLEET_SESSION", "")
        .env_remove("NORTH_FLEET_TEST")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .envs(sender.iter().copied())
        .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap()
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

fn shown(root: &Path) -> Value {
    let show = north(root, &["fleet", "show", "narrow", "--json"]);
    assert!(show.status.success(), "{}", String::from_utf8_lossy(&show.stderr));
    serde_json::from_slice(&show.stdout).unwrap()
}

#[test]
fn a_launch_returns_once_its_prompt_has_started_the_first_turn_even_behind_slow_session_hooks() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "slow-hooks");
    let launched = launch(root);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
    let show = shown(root);
    north(root, &["fleet", "forget", "narrow"]);
    assert_eq!(show["status"], "working", "{show}");
    assert!(show.to_string().contains("start the box"), "{show}");
}

#[test]
fn a_session_launched_from_a_claude_child_session_does_not_inherit_its_child_marker() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    let parent = [("CLAUDE_CODE_CHILD_SESSION", "1")];
    let launched = north_as(root, "here", &parent, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--system-prompt", &brief.display().to_string(), "start the box"]);
    let host = fs::read_to_string(root.join(".local/state/north/fleet/narrow/host.pid")).unwrap_or_default().trim().to_owned();
    let children = fs::read_to_string(format!("/proc/{host}/task/{host}/children")).unwrap_or_default();
    let environs: Vec<(String, String)> = std::iter::once(host.clone())
        .chain(children.split_whitespace().map(str::to_owned))
        .map(|pid| (pid.clone(), String::from_utf8_lossy(&fs::read(format!("/proc/{pid}/environ")).unwrap_or_default()).into_owned()))
        .collect();
    north(root, &["fleet", "forget", "narrow"]);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
    assert_eq!(environs.len(), 2, "the session host and its Claude child: {environs:?}");
    for (pid, environ) in &environs {
        let variables: Vec<&str> = environ.split('\0').collect();
        assert!(variables.contains(&"NORTH_FLEET_SESSION=narrow"), "{pid}: {variables:?}");
        assert!(!variables.iter().any(|variable| variable.starts_with("CLAUDE_CODE_CHILD_SESSION=")), "{pid}: {variables:?}");
    }
}

#[test]
fn a_launch_whose_first_turn_answers_not_logged_in_restarts_once_and_records_it() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "signed-out-once");
    let launched = launch(root);
    let show = shown(root);
    north(root, &["fleet", "forget", "narrow"]);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
    assert_eq!(show["status"], "working", "{show}");
    let noted = fs::read_to_string(root.join(".local/state/north/fleet/.credential-failures.jsonl")).unwrap_or_default();
    let rows: Vec<Value> = noted.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(rows.len(), 1, "{noted}");
    assert_eq!((rows[0]["session"].as_str(), rows[0]["account"].as_str()), (Some("narrow"), Some("a")));
}

#[test]
fn a_launch_whose_first_turn_answers_oauth_session_expired_is_recorded_and_refused_naming_the_account_without_a_restart() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "oauth-expired");
    let launched = launch(root);
    north(root, &["fleet", "forget", "narrow"]);
    let said = String::from_utf8_lossy(&launched.stderr);
    assert!(!launched.status.success(), "{said}");
    assert!(said.contains("narrow on claude:a answered its first turn with 'OAuth session expired'"), "{said}");
    let noted = fs::read_to_string(root.join(".local/state/north/fleet/.credential-failures.jsonl")).unwrap_or_default();
    let rows: Vec<Value> = noted.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(rows.len(), 1, "{noted}");
    assert_eq!((rows[0]["account"].as_str(), rows[0]["failure"].as_str()), (Some("a"), Some("oauth-expired")));
}

#[test]
fn a_host_holding_a_copy_of_another_hosts_login_refuses_to_launch_it_naming_the_owner_while_the_owner_launches() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let policy = root.join("op/orchestration.toml");
    let declared = fs::read_to_string(&policy).unwrap() + "[hosts.copy]\nserves = [\"warcraft\"]\n[accounts.claude]\nprofiles = { a = { here = {} } }\n";
    fs::write(&policy, declared).unwrap();

    let copy = launch_on(root, "copy");
    let listed = north_on(root, "copy", &["fleet", "accounts"]);
    let said = String::from_utf8_lossy(&copy.stderr);
    assert!(!copy.status.success(), "{said}");
    assert!(said.contains("claude:a's login belongs to here"), "{said}");
    assert!(!north_on(root, "copy", &["fleet", "show", "narrow"]).status.success(), "the copy host started a session on a");
    assert!(!String::from_utf8_lossy(&listed.stdout).contains("claude\ta"), "{}", String::from_utf8_lossy(&listed.stdout));

    let owner = launch(root);
    north(root, &["fleet", "forget", "narrow"]);
    assert!(owner.status.success(), "{}", String::from_utf8_lossy(&owner.stderr));
}

#[test]
fn a_launch_whose_cwd_names_another_machine_runs_there_on_a_label_only_that_machine_owns() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let policy = root.join("op/orchestration.toml");
    let declared = fs::read_to_string(&policy).unwrap() + "[hosts.far]\nssh = \"far\"\nresident_cap = 4\nserves = []\n[accounts.claude]\nprofiles = { a = { far = {} } }\n";
    fs::write(&policy, declared).unwrap();
    fs::write(root.join("placement-hosts.json"), serde_json::json!({"far": {"online": true, "live": 0, "accounts": ["a"]}}).to_string()).unwrap();
    executable(&root.join("bin/ssh"), "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec sh -c \"$*\"\n");
    let checkout = root.join("far-checkout");
    fs::create_dir_all(&checkout).unwrap();
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();

    let cwd = format!("far:{}", checkout.display());
    let launched = north(root, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--cwd", &cwd, "--system-prompt", &brief.display().to_string(), "start the box"]);
    let show = shown(root);
    north(root, &["fleet", "forget", "narrow"]);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
    assert_eq!((show["machine"].as_str(), show["status"].as_str()), (Some("far"), Some("working")), "{show}");
}

#[test]
fn sending_to_and_forgetting_a_session_whose_scope_is_gone_say_nothing_about_stopping_it() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let launched = launch(root);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
    let session = root.join(".local/state/north/fleet/narrow");
    let unit = fs::read_to_string(session.join("host.scope")).unwrap();
    let pid: i32 = fs::read_to_string(session.join("host.pid")).unwrap().trim().parse().unwrap();
    let stopped = Command::new(root.join("bin/systemctl")).args(["--user", "stop", unit.trim()]).env("FLEET_LAUNCH_FIXTURE", root).status().unwrap();
    assert!(stopped.success());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while Path::new(&format!("/proc/{pid}")).exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let sent = north(root, &["fleet", "send", "narrow", "carry on"]);
    let forgot = north(root, &["fleet", "forget", "narrow"]);
    for (verb, output) in [("send", &sent), ("forget", &forgot)] {
        let said = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{verb}: {said}");
        assert!(!said.contains("Failed to stop"), "{verb}: {said}");
    }
}

#[test]
fn a_send_names_its_sender_in_the_receivers_transcript_and_warns_a_second_sender_about_the_first() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let launched = launch(root);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));

    let first = north_as(root, "here", &[("CLAUDE_CODE_SESSION_ID", "28c1c1d1")], &["fleet", "send", "narrow", "staff every ready item"]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut shown = String::new();
    while !shown.contains("[from 28c1c1d1] staff every ready item") && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
        shown = String::from_utf8_lossy(&north(root, &["fleet", "show", "narrow"]).stdout).into_owned();
    }
    let second = north_as(root, "here", &[("NORTH_FLEET_SESSION", "proxy")], &["fleet", "send", "narrow", "launch nothing on a"]);
    let warned = String::from_utf8_lossy(&second.stderr).into_owned();
    north(root, &["fleet", "forget", "narrow"]);

    assert!(first.status.success() && second.status.success(), "{}{warned}", String::from_utf8_lossy(&first.stderr));
    assert!(!String::from_utf8_lossy(&first.stderr).contains("also messaged"), "{}", String::from_utf8_lossy(&first.stderr));
    assert!(shown.contains("user: [from 28c1c1d1] staff every ready item"), "{shown}");
    assert!(warned.contains("28c1c1d1 also messaged narrow 0m ago"), "{warned}");
}

#[test]
fn a_letter_relayed_from_another_host_names_its_sender_and_host_and_records_them() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let far = root.join("far");
    fs::create_dir_all(far.join(".local/state")).unwrap();
    executable(&root.join("bin/ssh"), "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nHOME=\"$FAR_HOME\" XDG_STATE_HOME=\"$FAR_HOME/.local/state\" exec sh -c \"$1\"\n");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), root.join("bin/north")).unwrap();
    let launched = launch(root);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));

    let far_home = far.to_str().unwrap();
    let far_state = format!("{far_home}/.local/state");
    north_as(root, "here", &[("HOME", far_home), ("XDG_STATE_HOME", &far_state)], &["fleet", "outbox", "take", "--for", "here"]);
    let posted = north_as(root, "here", &[("HOME", far_home), ("XDG_STATE_HOME", &far_state), ("NORTH_FLEET_SESSION", "wisp-lead")], &["fleet", "send", "narrow", "rolled back"]);
    let relayed = north_as(root, "here", &[("FAR_HOME", far_home)], &["fleet", "relay", "far"]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut shown = String::new();
    while !shown.contains("rolled back") && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
        shown = String::from_utf8_lossy(&north(root, &["fleet", "show", "narrow"]).stdout).into_owned();
    }
    let record: Value = serde_json::from_slice(&fs::read(root.join(".local/state/north/fleet/narrow/session.json")).unwrap()).unwrap();
    north(root, &["fleet", "forget", "narrow"]);

    assert!(String::from_utf8_lossy(&posted.stderr).contains("no session named narrow here; posted "), "{}", String::from_utf8_lossy(&posted.stderr));
    assert!(relayed.status.success(), "{}{}", String::from_utf8_lossy(&relayed.stdout), String::from_utf8_lossy(&relayed.stderr));
    let host = record["last_sender"]["name"].as_str().and_then(|name| name.strip_prefix("wisp-lead@")).unwrap_or_else(|| panic!("{record}"));
    assert!(shown.contains(&format!("user: [from wisp-lead@{host}] rolled back")), "{shown}");
}

#[test]
fn a_second_task_launch_on_a_held_item_is_refused_unless_it_says_why_it_runs_beside_the_first() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    let brief = brief.display().to_string();
    let task = |name: &str, extra: &[&str]| {
        let mut words = vec!["fleet", "launch", "--provider", "claude", "--account", "a", "--name", name, "--parent", "none", "--task", "smashcraft#4"];
        words.extend(extra);
        words.extend(["--system-prompt", &brief, "start the box"]);
        north(root, &words)
    };

    let first = task("narrow", &[]);
    let second = task("beside", &[]);
    let forced = task("forced", &["--force-dup", "a second judge of the same capture"]);
    let show = north(root, &["fleet", "show", "forced", "--json"]);
    for name in ["narrow", "beside", "forced"] {
        north(root, &["fleet", "forget", name]);
    }
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    let refused = String::from_utf8_lossy(&second.stderr);
    assert!(!second.status.success(), "{refused}");
    assert!(refused.contains("smashcraft#4 is held by fleet session narrow on here") && refused.contains("--force-dup"), "{refused}");
    assert!(forced.status.success(), "{}", String::from_utf8_lossy(&forced.stderr));
    let show: Value = serde_json::from_slice(&show.stdout).unwrap();
    assert!(show["placement"].as_str().is_some_and(|note| note.contains("dup: a second judge of the same capture")), "{show}");
}

fn launch_task(root: &Path, category: &str, extra: &[&str]) -> Output {
    let brief = root.join("box.md");
    fs::write(&brief, format!("Item: north#9 tier check\nCategory: {category}\nSurface: lua\nNeeds: warcraft\n")).unwrap();
    let brief = brief.display().to_string();
    let mut args = vec!["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--task", "north#9", "--system-prompt", &brief];
    args.extend(extra);
    args.push("start the box");
    north(root, &args)
}

#[test]
fn a_task_launch_starts_on_the_tier_its_briefs_category_earns_and_a_different_model_needs_a_reason() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let started = launch_task(root, "bug-known-cause", &[]);
    assert!(started.status.success(), "{}", String::from_utf8_lossy(&started.stderr));
    let known = shown(root);
    north(root, &["fleet", "forget", "narrow"]);
    assert_eq!((known["model"].as_str(), known["effort"].as_str()), (Some("claude-sonnet-5-5"), Some("high")), "{known}");
    assert!(known.to_string().contains("Tier: sonnet-high"), "{known}");

    let mechanical = launch_task(root, "mechanical", &[]);
    assert!(mechanical.status.success(), "{}", String::from_utf8_lossy(&mechanical.stderr));
    let cheap = shown(root);
    north(root, &["fleet", "forget", "narrow"]);
    assert_eq!(cheap["model"], "claude-haiku-5-5", "{cheap}");

    let refused = launch_task(root, "anim", &["--model", "opus"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--tier-reason"), "{}", String::from_utf8_lossy(&refused.stderr));
    assert!(!north(root, &["fleet", "show", "narrow", "--json"]).status.success(), "a refused launch starts no session");

    let allowed = launch_task(root, "anim", &["--model", "opus", "--tier-reason", "the cause spans two engines"]);
    assert!(allowed.status.success(), "{}", String::from_utf8_lossy(&allowed.stderr));
    let overridden = shown(root);
    north(root, &["fleet", "forget", "narrow"]);
    assert_eq!(overridden["model"], "opus", "{overridden}");
    assert!(overridden["placement"].as_str().unwrap_or_default().contains("tier: the cause spans two engines"), "{overridden}");
}

const FAKE_GH: &str = "#!/bin/sh\ncase \"$*\" in\n  *\"state createdAt\"*) printf '{\"data\":{\"r0\":{\"i4\":{\"state\":\"OPEN\",\"createdAt\":\"2026-01-02T00:00:00Z\"}}}}\\n' ;;\n  *) echo '[]' ;;\nesac\n";

fn held_by_name(root: &Path) -> Vec<String> {
    let listed = north(root, &["threads", "list", "--json"]);
    assert!(listed.status.success(), "{}", String::from_utf8_lossy(&listed.stderr));
    let claims: Vec<Value> = serde_json::from_slice(&listed.stdout).unwrap();
    claims.iter().map(|claim| format!("{} {}", claim["item"].as_str().unwrap(), claim["owner"].as_str().unwrap())).collect()
}

#[test]
fn a_task_launch_holds_its_item_in_the_threads_ledger_under_the_session_name_until_the_session_stops() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    executable(&root.join("bin/gh"), FAKE_GH);
    std::os::unix::fs::symlink(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/threads"), root.join("bin/threads")).unwrap();
    let schema = north(root, &["threads", "release", "smashcraft#4", "--by", "setup"]);
    assert!(String::from_utf8_lossy(&schema.stderr).contains("has no owner"), "{}", String::from_utf8_lossy(&schema.stderr));
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    let brief = brief.display().to_string();

    let launched = north(root, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--task", "smashcraft#4", "--system-prompt", &brief, "start the box"]);
    assert!(launched.status.success() && !String::from_utf8_lossy(&launched.stderr).contains("does not hold"), "{}", String::from_utf8_lossy(&launched.stderr));
    let held = held_by_name(root);
    let refused = north(root, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "beside", "--parent", "none", "--task", "smashcraft#4", "--system-prompt", &brief, "start the box"]);
    let stopped = north(root, &["fleet", "stop", "narrow"]);
    let after = held_by_name(root);
    north(root, &["fleet", "forget", "narrow"]);

    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
    assert_eq!(held, ["smashcraft#4 narrow"]);
    assert!(!refused.status.success() && String::from_utf8_lossy(&refused.stderr).contains("smashcraft#4 is held by"), "{}", String::from_utf8_lossy(&refused.stderr));
    assert!(stopped.status.success(), "{}", String::from_utf8_lossy(&stopped.stderr));
    assert!(after.is_empty(), "{after:?}");
}

const SSH_TO_FAR: &str = "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nHOME=\"$FAR_HOME\" XDG_STATE_HOME=\"$FAR_HOME/.local/state\" exec sh -c \"$1\"\n";

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

struct TwoHosts {
    root: tempfile::TempDir,
}

impl TwoHosts {
    fn new() -> Self {
        let hosts = Self { root: tempfile::tempdir().unwrap() };
        let (here, far) = (hosts.here(), hosts.far());
        fixture(here, "idle-interrupt");
        fs::create_dir_all(far.join(".local/state")).unwrap();
        fixture(&far, "done");
        executable(&here.join("bin/ssh"), SSH_TO_FAR);
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), here.join("bin/north")).unwrap();
        hosts
    }

    fn here(&self) -> &Path {
        self.root.path()
    }

    fn far(&self) -> std::path::PathBuf {
        self.root.path().join("far")
    }

    fn relay(&self) -> Output {
        let far = self.far();
        north_as(self.here(), "here", &[("FAR_HOME", far.to_str().unwrap())], &["fleet", "relay", "far"])
    }

    fn launch_boss_here_and_kid_far(&self) {
        let launched = launch_named(self.here(), "boss", "none");
        assert!(launched.status.success(), "{}", text(&launched.stderr));
        let armed = north(&self.far(), &["fleet", "outbox", "take", "--for", "here"]);
        assert!(armed.status.success(), "{}", text(&armed.stderr));
        let kid = launch_named(&self.far(), "kid", "boss");
        assert!(kid.status.success(), "{}", text(&kid.stderr));
    }
}

impl Drop for TwoHosts {
    fn drop(&mut self) {
        north(self.here(), &["fleet", "forget", "boss"]);
        north(&self.far(), &["fleet", "forget", "kid"]);
    }
}

fn launch_named(root: &Path, name: &str, parent: &str) -> Output {
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    north(root, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", name, "--parent", parent, "--system-prompt", &brief.display().to_string(), "start the box"])
}

#[test]
fn a_session_launched_on_another_host_for_a_parent_here_delivers_its_final_report_to_that_parent() {
    let hosts = TwoHosts::new();
    hosts.launch_boss_here_and_kid_far();

    until("the report in the parent's transcript", || {
        hosts.relay();
        text(&north(hosts.here(), &["fleet", "show", "boss"]).stdout).contains("[from kid · turn 1 · exit completed] Done: fake report")
    });
    let shown = text(&north(hosts.here(), &["fleet", "show", "boss"]).stdout);
    assert!(shown.contains("user: [from "), "{shown}");
    let letters = fs::read_dir(hosts.far().join(".local/state/north/fleet/.outbox")).unwrap().flatten().filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json")).count();
    assert_eq!(letters, 0, "a relayed report is dropped from the host that posted it");
}

#[test]
fn north_fleet_list_and_tree_here_show_a_session_registered_on_another_host_with_its_parent() {
    let hosts = TwoHosts::new();
    hosts.launch_boss_here_and_kid_far();

    let relayed = hosts.relay();
    assert!(relayed.status.success(), "{}{}", text(&relayed.stdout), text(&relayed.stderr));
    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap().trim().to_owned();
    let list = text(&north(hosts.here(), &["fleet", "list"]).stdout);
    let kid = list.lines().find(|line| line.starts_with("kid\t")).unwrap_or_else(|| panic!("{list}"));
    let columns: Vec<&str> = kid.split('\t').collect();
    assert_eq!((columns[1], columns[2], columns[5]), ("claude", "a", "boss"), "{kid}");
    assert!(columns[4].starts_with(&format!("{host}:")), "{kid}");
    let tree = text(&north(hosts.here(), &["fleet", "tree"]).stdout);
    let boss = tree.lines().position(|line| line.starts_with("boss ")).unwrap_or_else(|| panic!("{tree}"));
    assert!(tree.lines().nth(boss + 1).is_some_and(|line| line.starts_with("  kid ") && line.ends_with(&format!("on {host}"))), "{tree}");
}

#[test]
fn a_gate_a_session_opens_on_another_host_is_listed_here_by_host_qualified_id_and_answered_through_it() {
    let hosts = TwoHosts::new();
    hosts.launch_boss_here_and_kid_far();
    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap().trim().to_owned();

    let opened = north(&hosts.far(), &["gate", "open", "--session", "kid", "--question", "Rent the box?", "--option", "yes", "--option", "no", "--recommend", "1", "--default", "2", "--default-after", "60s"]);
    assert!(opened.status.success(), "{}", text(&opened.stderr));
    let id = text(&opened.stdout).trim().to_owned();
    let label = format!("{host}:{id}");

    until("the gate in this host's gate list", || {
        hosts.relay();
        text(&north(hosts.here(), &["gate", "list"]).stdout).contains(&format!("{label}\tkid\tRent the box?\topen"))
    });
    until("the parent told of the gate by its qualified id", || {
        hosts.relay();
        text(&north(hosts.here(), &["fleet", "show", "boss"]).stdout).contains(&format!("[gate {label} from kid]"))
    });
    let far = hosts.far();
    let answered = north_as(hosts.here(), "here", &[("FAR_HOME", far.to_str().unwrap())], &["gate", "answer", &label, "1"]);
    assert!(answered.status.success(), "{}", text(&answered.stderr));
    assert!(text(&answered.stdout).contains("closed by tom: yes"), "{}", text(&answered.stdout));
    until("the answer in the asking session's transcript", || text(&north(&hosts.far(), &["fleet", "show", "kid"]).stdout).contains("Tom answered: yes"));
}

#[test]
fn a_prompt_queued_to_a_finished_worker_just_before_its_host_stops_runs_on_the_restarted_host_without_a_send_or_revive() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "done");
    assert!(launch_named(root, "boss", "none").status.success());
    assert!(launch_named(root, "kid", "boss").status.success());
    let session = root.join(".local/state/north/fleet/kid");
    let stream = || fs::read_to_string(session.join("g1/stream.jsonl")).unwrap_or_default();
    until("the worker's final report", || stream().contains("Done: fake report"));

    let first_host = fs::read_to_string(session.join("host.pid")).unwrap();
    let sent = north(root, &["fleet", "send", "kid", "stall this prompt, then run it on the next host"]);
    let unit = fs::read_to_string(session.join("host.scope")).unwrap();
    let stopped = Command::new(root.join("bin/systemctl")).args(["--user", "stop", unit.trim()]).env("FLEET_LAUNCH_FIXTURE", root).status().unwrap();

    let ran = std::panic::catch_unwind(|| until("the restarted worker to run the prompt", || stream().matches("Done: fake report").count() >= 2));
    let restarted = fs::read_to_string(session.join("host.pid")).unwrap();
    let record = fs::read_to_string(session.join("session.json")).unwrap();
    for name in ["kid", "boss"] {
        north(root, &["fleet", "forget", name]);
    }

    assert!(sent.status.success() && stopped.success(), "{}", text(&sent.stderr));
    assert!(ran.is_ok(), "the prompt never ran after the host stopped");
    assert_ne!(first_host, restarted, "a new host replaced the stopped one");
    assert!(record.contains(r#""stopped": false"#), "{record}");
}

#[test]
fn a_message_sent_right_after_reviving_a_parked_session_shows_it_working_at_once_and_starts_a_turn_on_that_message() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "done");
    assert!(launch_named(root, "kid", "none").status.success());
    let session = root.join(".local/state/north/fleet/kid");
    let stream = || fs::read_to_string(session.join("g1/stream.jsonl")).unwrap_or_default();
    until("the worker's final report", || stream().contains("Done: fake report"));
    let unit = fs::read_to_string(session.join("host.scope")).unwrap();
    let parked = Command::new(root.join("bin/systemctl")).args(["--user", "stop", unit.trim()]).env("FLEET_LAUNCH_FIXTURE", root).status().unwrap();
    let status = || {
        let listed: Value = serde_json::from_slice(&north(root, &["fleet", "list", "--json"]).stdout).unwrap_or_default();
        listed.as_array().into_iter().flatten().find(|session| session["name"] == "kid").map(|session| session["status"].to_string()).unwrap_or_default()
    };
    until("the worker parked", || status() == r#""parked""#);
    executable(&root.join("bin/claude"), &format!("#!/bin/sh\nFAKE_CLAUDE_HOOK_MS=5000 exec {} --trace slow-hooks \"$@\"\n", support::fake_claude()));

    let revived = north(root, &["fleet", "revive", "kid"]);
    let sent = north(root, &["fleet", "send", "kid", "resume from your handoff"]);
    let at_once = status();
    let ran = std::panic::catch_unwind(|| {
        until("a turn on the message", || {
            let frames: Vec<Value> = stream().lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
            let echoed = frames.iter().position(|frame| frame["isReplay"] == true && frame["message"].to_string().contains("resume from your handoff"));
            echoed.is_some_and(|at| frames[at..].iter().any(|frame| frame["type"] == "assistant"))
        })
    });
    north(root, &["fleet", "forget", "kid"]);

    assert!(parked.success() && revived.status.success() && sent.status.success(), "{}{}", text(&revived.stderr), text(&sent.stderr));
    assert_eq!(at_once, r#""working""#, "a revived session still loading its conversation holds the message it was sent");
    assert!(ran.is_ok(), "the revived session never ran the message");
}

#[test]
fn a_launch_that_names_no_issue_is_refused_a_model_without_a_tier_reason_and_runs_it_with_one() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let untasked = |brief: &str, extra: &[&str]| {
        let path = root.join("box.md");
        fs::write(&path, brief).unwrap();
        let path = path.display().to_string();
        let mut args = vec!["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--system-prompt", &path];
        args.extend(extra);
        args.push("start the work");
        north(root, &args)
    };

    let refused = untasked("Delegation: role=worker\nCategory: tooling\n", &["--model", "opus"]);
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{stderr}");
    assert!(stderr.contains("--model opus") && stderr.contains("Tier-reason:"), "{stderr}");
    assert!(!north(root, &["fleet", "show", "narrow", "--json"]).status.success(), "a refused launch starts no session");

    let reasoned = untasked("Delegation: role=worker\nCategory: tooling\nTier-reason: a wrong routing rule misplaces every later launch\n", &["--model", "opus"]);
    assert!(reasoned.status.success(), "{}", String::from_utf8_lossy(&reasoned.stderr));
    let session = shown(root);
    north(root, &["fleet", "forget", "narrow"]);
    assert_eq!(session["model"], "opus", "{session}");
    assert!(session["placement"].as_str().unwrap_or_default().contains("tier: a wrong routing rule misplaces every later launch"), "{session}");

    let unnamed = untasked("Delegation: role=worker\n", &[]);
    assert!(unnamed.status.success(), "a launch that names no model needs no reason: {}", String::from_utf8_lossy(&unnamed.stderr));
    north(root, &["fleet", "forget", "narrow"]);
}
