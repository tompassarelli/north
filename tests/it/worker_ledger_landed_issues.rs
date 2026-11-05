use north_core::foundation::shim;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const FLEET: &str = r#"[{"name":"sdonly-1127b","account":"d","conversation":"sdonly","parent":null,"task_ref":"makgora#1127","effort":"medium","created_ms":0},{"name":"flaky-shared-target","account":"d","conversation":"flaky","parent":null,"task_ref":"north#437","effort":"medium","created_ms":0},{"name":"settle-transient-bound","account":"d","conversation":"settle","parent":null,"task_ref":"north#308","effort":"medium","created_ms":0},{"name":"queued-600","account":"d","conversation":"queued","parent":null,"task_ref":"north#600","effort":"medium","created_ms":0},{"name":"offload-runtime-pin","account":"d","conversation":"offload","parent":null,"task_ref":"north#482","effort":"medium","created_ms":0},{"name":"north-88-d0020","account":"d","conversation":"silent","parent":null,"task_ref":"north#88","effort":"medium","created_ms":0},{"name":"b-side","account":"b","conversation":"bside","parent":null,"task_ref":"north#700","effort":"medium","created_ms":0}]"#;
const PUSHED: &str = "1791425100";

fn rig(root: &Path) {
    fs::create_dir_all(root.join("bin")).unwrap();
    let script = format!("case \"$1\" in threads) exec {} \"$@\";; esac\ncase \"$1 $2\" in \"fleet list\") printf '%s\\n' '{FLEET}'; exit 0;; esac\nprintf '[]\\n'\n", env!("CARGO_BIN_EXE_north"));
    shim::write_script(&root.join("bin/north"), &script);
}

fn git(repo: &Path, at: &str, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .env("GIT_AUTHOR_DATE", at)
        .env("GIT_COMMITTER_DATE", at)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &Path, at: &str, subject: &str) -> String {
    git(repo, at, &["commit", "-q", "--allow-empty", "-m", subject]);
    git(repo, at, &["rev-parse", "HEAD"])
}

fn checkout(root: &Path, name: &str) -> std::path::PathBuf {
    let repo = root.join("code").join(name).join("main");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, "2026-10-07T00:00:00Z", &["init", "-q", "-b", "main"]);
    commit(&repo, "2026-10-07T00:00:00Z", "Base");
    repo
}

fn transcript(root: &Path, conversation: &str, item: &str, report: &str) {
    let repo = item.split('#').next().unwrap();
    let path = root.join(format!("claude/projects/-home-tom-code-{repo}-main/{conversation}.jsonl"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let user = format!(r#"{{"type":"user","timestamp":"2026-10-08T01:00:00Z","sessionId":"{conversation}","message":{{"content":"Item: {item}\nCategory: debugging-unknown-cause"}}}}"#);
    let assistant = format!(r#"{{"type":"assistant","timestamp":"2026-10-08T01:00:30Z","requestId":"r-{conversation}","message":{{"model":"claude-opus-5-5","usage":{{"output_tokens":50,"input_tokens":700}},"content":[{{"type":"text","text":"{report}"}}]}}}}"#);
    fs::write(path, format!("{user}\n{assistant}\n")).unwrap();
}

fn north(root: &Path, args: &[&str]) -> Output {
    let repos = format!("{} {}", root.join("code/north/main").display(), root.join("code/makgora/main").display());
    Command::new(env!("CARGO_BIN_EXE_north"))
        .args(args)
        .env("NORTH_HOME", env!("CARGO_MANIFEST_DIR"))
        .env("NORTH_BIN", root.join("bin/north"))
        .env("HOME", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("THREADS_DB", root.join("threads.db"))
        .env("CLAUDE_CONFIG_DIR", root.join("claude"))
        .env("CODEX_CONFIG_DIR", root.join("codex"))
        .env("AGY_CONFIG_DIR", root.join("agy"))
        .env("AGENTS_ORCHESTRATION", root.join("absent.toml"))
        .env("LEDGER_REPOS", repos)
        .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap()
}

fn run_of(root: &Path, agent: &str) -> String {
    let query = format!("select item, outcome, landed, landed_sha from runs where agent = '{agent}'");
    let output = Command::new("sqlite3").arg(root.join("threads.db")).arg(query).output().unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn a_lane_autoland_or_another_hosts_lander_took_to_main_lands_its_run_and_counts_in_the_accounts_landed_issues() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    rig(root);
    let makgora = checkout(root, "makgora");
    let fast_forwarded = commit(&makgora, "2026-10-08T01:40:00Z", "Ship the sd-only path (makgora#1127)");
    git(&makgora, "2026-10-08T01:40:00Z", &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let north_repo = checkout(root, "north");
    git(&north_repo, "2026-10-08T01:30:00Z", &["checkout", "-q", "-b", "flaky-shared-target"]);
    let lane_tip = commit(&north_repo, "2026-10-08T01:30:00Z", "Share one target dir between flaky tests (north#437)");
    let batched_tip = commit(&north_repo, "2026-10-08T01:35:00Z", "Pin the offload runtime (north#482)");
    git(&north_repo, "2026-10-08T01:30:00Z", &["checkout", "-q", "main"]);
    commit(&north_repo, "2026-10-08T01:50:00Z", "Another lane (north#12)");
    let rebased = commit(&north_repo, "2026-10-08T02:10:00Z", "Share one target dir between flaky tests (north#437)");
    let settled = commit(&north_repo, "2026-10-08T02:20:00Z", "Settle the transient bound, box 2 (north#308)");
    let batched = commit(&north_repo, "2026-10-08T02:00:00Z", "Pin the offload runtime (north#482)");
    let batch_tip = commit(&north_repo, "2026-10-08T02:01:00Z", "Watch the runtime canary (north#482)");
    let queued = commit(&north_repo, "2026-10-08T02:30:00Z", "Queue the change (north#600)");
    git(&north_repo, "2026-10-08T02:30:00Z", &["update-ref", "refs/remotes/origin/main", "HEAD"]);

    transcript(root, "sdonly", "makgora#1127", "Done: the sd-only path is queued on Autoland.");
    transcript(root, "flaky", "north#437", "Done: the lane is queued for the Nexus lander.");
    transcript(root, "settle", "north#308", "Not done: box 2 landed and is ticked; boxes 3-4 keep north#308 open.");
    transcript(root, "queued", "north#600", "Not done: the change is queued.");
    transcript(root, "offload", "north#482", "Done: the runtime pin is queued for the lander.");
    transcript(root, "bside", "north#700", "Done: landed.");
    fs::create_dir_all(root.join("state/safe-push")).unwrap();
    let rows = [
        format!("{PUSHED}\tmakgora\tclaude/land-sdonly-1127b-{}\t{fast_forwarded}\t\tsdonly-1127b", &fast_forwarded[..12]),
        format!("{PUSHED}\tnorth\tflaky-shared-target\t{lane_tip}\t\tflaky-shared-target"),
        format!("{PUSHED}\tnorth\tsettle-transient-bound\t{settled}\t{settled}\tsettle-transient-bound"),
        format!("{PUSHED}\tnorth\tqueued-600\t{queued}\t{queued}\tqueued-600"),
        format!("{PUSHED}\tnorth\toffload-runtime-pin\t{batched_tip}\t{batch_tip}\toffload-runtime-pin"),
        format!("{PUSHED}\tnorth\tnorth-88-d0020\t{queued}\t{queued}\tnorth-88-d0020"),
        format!("{PUSHED}\tnorth\tb-side\t{queued}\t{queued}\tb-side"),
    ];
    fs::write(root.join("state/safe-push/pushes.tsv"), rows.join("\n") + "\n").unwrap();

    let recorded = north(root, &["worker-ledger", "--since", "2026-10-08"]);
    assert!(recorded.status.success(), "{}", String::from_utf8_lossy(&recorded.stderr));
    assert_eq!(run_of(root, "sdonly"), format!("makgora#1127|done|1|{fast_forwarded}"), "an Autoland queue push lands with the tip that reached main");
    assert_eq!(run_of(root, "flaky"), format!("north#437|done|1|{rebased}"), "a lane another host's lander rebased lands with the rebased commit");
    assert_eq!(run_of(root, "offload"), format!("north#482|done|1|{batched}"), "a lane the lander cherry-picked before its checks lands with its own commit, dated before the logged push, not the batch tip");

    let measured = north(root, &["worker-ledger", "--landed-issues", "d", "--day", "2026-10-08"]);
    assert!(measured.status.success(), "{}", String::from_utf8_lossy(&measured.stderr));
    let expected = format!(
        "account d on 2026-10-08:\ndone makgora#1127 sdonly-1127b {}\ndone north#308 settle-transient-bound {}\ndone north#437 flaky-shared-target {}\ndone north#482 offload-runtime-pin {}\nmiss north#600 queued-600 {} (landed, Not done without its box ticked)\nmiss north#88 north-88-d0020 {} (no run row)\n4 of 6 landed issues have a done-and-landed run (66%)\n",
        &fast_forwarded[..9],
        &settled[..9],
        &rebased[..9],
        &batched[..9],
        &queued[..9],
        &queued[..9]
    );
    assert_eq!(String::from_utf8(measured.stdout).unwrap(), expected);
}
