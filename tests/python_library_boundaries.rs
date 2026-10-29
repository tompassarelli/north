use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

fn lib() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("harness/lib")
}

fn python(script: &str, arguments: &[&Path], envs: &[(&str, &Path)]) -> Output {
    Command::new("python3")
        .args(["-I", "-c", script])
        .arg(lib())
        .args(arguments)
        .envs(envs.iter().copied())
        .output()
        .unwrap()
}

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

const GOAL_RECORD: &str = "import sys\nfrom pathlib import Path\nsys.path.insert(0, sys.argv[1])\nimport goal_eta\n\
goal_eta.LIB = Path(sys.argv[2])\nassert goal_eta.record({}) is None";

#[test]
fn a_calibration_that_outlives_its_timeout_warns_once_and_records_nothing() {
    let scratch = TempDir::new().unwrap();
    fs::create_dir_all(scratch.path().join("goal/harness/lib")).unwrap();
    let north = scratch.path().join("north");
    executable(&north, "#!/usr/bin/env bash\n[[ \"$1 $2 $3\" == \"threads calibrate --write\" ]] || exit 97\nexec sleep 6\n");
    let output = python(GOAL_RECORD, &[&scratch.path().join("goal/harness/lib")], &[("PYTHONDONTWRITEBYTECODE", Path::new("1")), ("NORTH_BIN", &north)]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim_end(),
        "warning: threads calibrate --write timed out after 5s; goal ETA row not recorded"
    );
}

const HOSTED: &str = "import sys\nsys.path.insert(0, sys.argv[1])\nimport sqlite3, usage\n\
row = lambda account, pct: dict(zip(usage.COLUMNS, ['2026-10-15T12:00:00Z', 'claude', account, 'window', 'session', pct, 300, '2026-10-15T15:00:00Z', None, 'fixture']))\n\
db = sqlite3.connect(':memory:')\n\
usage.record(db, [row('a', 20), row('b', 30), row('c', 92), dict(row('x', 5), provider='gemini')] * 2)\n\
print(sorted((r['provider'], r['account']) for r in usage.latest(db)), db.execute('SELECT count(*) FROM usage').fetchone()[0])";

#[test]
fn a_claude_account_with_no_profile_on_any_host_is_left_out_of_usage_and_one_on_another_host_stays() {
    let scratch = TempDir::new().unwrap();
    let hostname = String::from_utf8(Command::new("hostname").output().unwrap().stdout).unwrap();
    let orchestration = scratch.path().join("orchestration.toml");
    fs::write(&orchestration, format!("[accounts.claude.profiles]\na = {{ \"{}\" = {{}} }}\nb = {{ elsewhere = {{}} }}\n", hostname.trim())).unwrap();
    let output = python(HOSTED, &[], &[("AGENTS_ORCHESTRATION", &orchestration)]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        "[('claude', 'a'), ('claude', 'b'), ('gemini', 'x')] 4"
    );
}

const BOUNDARY: &str = "import sys, sqlite3\nfrom pathlib import Path\nsys.path.insert(0, sys.argv[1])\nimport usage\n\
rows = [dict(zip(usage.COLUMNS, ['2026-10-15T12:00:00Z', 'gemini', 'x1', 'window', 'week', 40, 10080, '2026-10-17T04:00:00Z', None, 'fixture']))]\n\
mirror = Path(usage.DB)\n\
with sqlite3.connect(mirror) as db:\n    usage.record(db, rows)\n\
frozen = mirror.with_name('threads.db')\n\
with sqlite3.connect(frozen) as db:\n    usage.record(db, [dict(rows[0], used_pct=90)])\n\
frozen_bytes = frozen.read_bytes()\n\
with usage.open_readout_db() as db:\n    assert usage.latest(db)[0]['used_pct'] == 40\n    usage.publish(db, [dict(rows[0], ts='2026-10-15T12:05:00Z', used_pct=45)])\n    assert usage.latest(db)[0]['used_pct'] == 45\n\
with sqlite3.connect(mirror.as_uri() + '?mode=ro', uri=True) as db:\n    assert usage.latest(db)[0]['used_pct'] == 45\n\
assert frozen.read_bytes() == frozen_bytes\nprint('boundary ok')";

#[test]
fn a_remote_client_reads_the_snapshot_then_publishes_to_the_authoritative_ledger_and_its_mirror() {
    let scratch = TempDir::new().unwrap();
    let root = scratch.path();
    for directory in ["home/.config/threads", "home/.local/state/threads", "remote-home", "bin", "op"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    fs::write(root.join("home/.config/threads/remote"), "fixture-host\n").unwrap();
    fs::write(root.join("op/costs.toml"), "").unwrap();
    executable(
        &root.join("bin/ssh"),
        "#!/usr/bin/env bash\ncase \"${!#}\" in\n  *'north threads ingest') HOME=\"$REMOTE_HOME\" THREADS_DB=\"$REMOTE_DB\" \"$THREADS_BIN\" ingest ;;\n  *'north threads snapshot-pages') HOME=\"$REMOTE_HOME\" THREADS_DB=\"$REMOTE_DB\" \"$NORTH_BIN\" threads snapshot-pages ;;\n  *) exit 97 ;;\nesac\n",
    );
    let path = format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::new("python3")
        .args(["-I", "-c", BOUNDARY])
        .arg(lib())
        .env_remove("THREADS_DB")
        .env("HOME", root.join("home"))
        .env("PATH", path)
        .env("NORTH_BIN", env!("CARGO_BIN_EXE_north"))
        .env("AGENTS_OPERATOR_DIR", root.join("op"))
        .env("REMOTE_HOME", root.join("remote-home"))
        .env("REMOTE_DB", root.join("remote.db"))
        .env("THREADS_BIN", Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/threads"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("boundary ok"));
}
