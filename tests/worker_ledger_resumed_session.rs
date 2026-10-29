use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const FLEET: &str = r#"[{"name":"d-ledgersee","conversation":"resumed","parent":null,"task_ref":"north#255","effort":"medium"},{"name":"d-parked","conversation":"parked","parent":null,"task_ref":"north#252","effort":"medium"}]"#;

fn rig(root: &Path) {
    fs::create_dir_all(root.join("bin")).unwrap();
    let north = root.join("bin/north");
    let script = format!("#!/bin/sh\ncase \"$1\" in threads) exec {} \"$@\";; esac\ncase \"$1 $2\" in \"fleet list\") printf '%s\\n' '{FLEET}'; exit 0;; esac\nprintf '[]\\n'\n", env!("CARGO_BIN_EXE_north"));
    fs::write(&north, script).unwrap();
    fs::set_permissions(&north, fs::Permissions::from_mode(0o755)).unwrap();
}

fn turn(path: &Path, at: &str, prompt: &str, report: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let user = format!(r#"{{"type":"user","timestamp":"2026-10-08T{at}:00Z","sessionId":"resumed","message":{{"content":"{prompt}"}}}}"#);
    let assistant = format!(
        r#"{{"type":"assistant","timestamp":"2026-10-08T{at}:30Z","requestId":"r{at}","message":{{"model":"claude-opus-5-5","usage":{{"output_tokens":50,"input_tokens":700}},"content":[{{"type":"text","text":"{report}"}}]}}}}"#
    );
    let mut file = OpenOptions::new().create(true).append(true).open(path).unwrap();
    writeln!(file, "{user}\n{assistant}").unwrap();
}

fn record(root: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["worker-ledger", "--since", "2026-10-08"])
        .env("NORTH_HOME", env!("CARGO_MANIFEST_DIR"))
        .env("NORTH_BIN", root.join("bin/north"))
        .env("HOME", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("THREADS_DB", root.join("threads.db"))
        .env("CLAUDE_CONFIG_DIR", root.join("claude"))
        .env("CODEX_CONFIG_DIR", root.join("codex"))
        .env("AGY_CONFIG_DIR", root.join("agy"))
        .env("AGENTS_ORCHESTRATION", root.join("absent.toml"))
        .env("LEDGER_REPOS", root.join("no-repos"))
        .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

fn run(root: &Path) -> String {
    run_of(root, "resumed")
}

fn run_of(root: &Path, agent: &str) -> String {
    let query = format!("select item, outcome, ended, minutes, substr(report, 1, 12), landed, landed_sha, landed_at from runs where agent = '{agent}'");
    let output = Command::new("sqlite3").arg(root.join("threads.db")).arg(query).output().unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn settled(root: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["threads", "run-ids", "--settled"])
        .env("NORTH_HOME", env!("CARGO_MANIFEST_DIR"))
        .env("HOME", root)
        .env("THREADS_DB", root.join("threads.db"))
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn a_settled_fleet_session_that_lands_in_a_later_turn_is_reread_and_recorded_done_and_landed_by_its_push() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    rig(root);
    let transcript = root.join("claude/projects/-home-tom-code-north-main/resumed.jsonl");
    turn(&transcript, "01:00", "Item: north#255 ledger", "Not done: the change is built and queued for the land slot.");
    record(root);
    assert_eq!(run(root), "north#255|not done|2026-10-08T01:00:30Z|0|Not done: th|0||");
    assert!(settled(root).lines().any(|id| id == "resumed"), "the first report settles the run");

    turn(&transcript, "02:00", "your turn to land", "Done: landed as 1234abcd on origin/main.");
    fs::create_dir_all(root.join("state/safe-push")).unwrap();
    let push = "1791425100\tnorth\td-ledgersee\t1234abcd\t1234abcd\td-ledgersee\treviewer\n";
    fs::write(root.join("state/safe-push/pushes.tsv"), push).unwrap();
    record(root);
    assert_eq!(run(root), "north#255|done|2026-10-08T02:00:30Z|60|Done: landed|1|1234abcd|2026-10-08T02:05:00Z");

    turn(&transcript, "03:00", "spawn hold notice", "Noted.");
    record(root);
    assert_eq!(run(root), "north#255|done|2026-10-08T03:00:30Z|120|Done: landed|1|1234abcd|2026-10-08T02:05:00Z", "a notice acknowledged after the report keeps it");

    turn(&transcript, "04:00", "<task-notification>the build finished</task-notification>", "The build finished and its log is clean.");
    record(root);
    assert_eq!(run(root), "north#255|done|2026-10-08T04:00:30Z|180|Done: landed|1|1234abcd|2026-10-08T02:05:00Z", "an unclear reply after the report keeps it");
}

#[test]
fn a_recorded_fleet_session_parked_mid_turn_takes_the_landing_the_land_queue_pushed_under_its_name() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    rig(root);
    let transcript = root.join("claude/projects/-home-tom-code-north-main/parked.jsonl");
    turn(&transcript, "01:00", "Item: north#252 restart", "Done: the fix is reviewed and queued for the land slot.");
    record(root);
    let tool = r#"{"type":"assistant","timestamp":"2026-10-08T01:10:00Z","requestId":"r2","message":{"model":"claude-opus-5-5","usage":{"output_tokens":5,"input_tokens":700},"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"sleep 60"}}]}}"#;
    let mut file = OpenOptions::new().append(true).open(&transcript).unwrap();
    writeln!(file, r#"{{"type":"user","timestamp":"2026-10-08T01:09:00Z","sessionId":"parked","message":{{"content":"park"}}}}"#).unwrap();
    writeln!(file, "{tool}").unwrap();
    fs::create_dir_all(root.join("state/safe-push")).unwrap();
    fs::write(root.join("state/safe-push/pushes.tsv"), "1791425100\tnorth\td-parked\t5678ef01\t5678ef01\td-parked\treviewer\n").unwrap();
    record(root);
    assert_eq!(run_of(root, "parked"), "north#252|done|2026-10-08T01:00:30Z|0|Done: the fi|1|5678ef01|2026-10-08T02:05:00Z");
}
