use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn threads(root: &Path, verb: &str) -> Command {
    let mut command = Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/threads"));
    command
        .arg(verb)
        .env("HOME", root)
        .env("THREADS_DB", root.join("threads.db"))
        .env("NORTH_BIN", env!("CARGO_BIN_EXE_north"))
        .env("PATH", std::env::var("PATH").unwrap_or_default());
    command
}

fn ingest(root: &Path, row: &str) {
    let mut child = threads(root, "ingest").stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(row.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

fn outcome(root: &Path, id: &str) -> String {
    let query = format!("select outcome from runs where agent='{id}'");
    let output = Command::new("sqlite3").arg(root.join("threads.db")).arg(query).output().unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn row(id: &str, outcome: &str, backfill: bool) -> String {
    format!(
        r#"{{"id":"{id}","backfill":{backfill},"ended":"2026-10-09T00:00:00Z","tier":"haiku","actual_min":5,"outcome":"{outcome}","brief":"Item: smashcraft#7 Category: native-check"}}"#
    ) + "\n"
}

#[test]
fn the_runs_table_takes_an_idle_outcome_and_a_backfill_relabels_only_a_recorded_unclear_run() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    assert!(threads(root, "run-ids").status().unwrap().success());
    for (id, recorded) in [("quiet", "idle"), ("vague", "unclear"), ("settled", "done")] {
        ingest(root, &row(id, recorded, false));
    }
    assert_eq!(outcome(root, "quiet"), "idle");
    for (id, reread) in [("quiet", "done"), ("vague", "done"), ("settled", "not done")] {
        ingest(root, &row(id, reread, true));
    }
    let relabelled: Vec<String> = ["quiet", "vague", "settled"].iter().map(|id| outcome(root, id)).collect();
    assert_eq!(relabelled, ["idle", "done", "done"]);
}

fn row_ending(id: &str, outcome: &str, ended: &str) -> String {
    format!(
        r#"{{"id":"{id}","backfill":true,"status":"finished","ended":"{ended}","tier":"haiku","actual_min":9,"outcome":"{outcome}","report":"{outcome} report","brief":"Item: smashcraft#7 Category: native-check"}}"#
    ) + "\n"
}

fn field(root: &Path, id: &str, column: &str) -> String {
    let query = format!("select {column} from runs where agent='{id}'");
    let output = Command::new("sqlite3").arg(root.join("threads.db")).arg(query).output().unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn a_recorded_worker_that_kept_working_takes_its_later_end_outcome_and_report_and_run_ids_name_its_recorded_end() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    assert!(threads(root, "run-ids").status().unwrap().success());
    ingest(root, &row_ending("long", "not done", "2026-10-09T00:00:00Z"));
    ingest(root, &row_ending("long", "done", "2026-10-09T06:00:00Z"));
    assert_eq!((field(root, "long", "outcome"), field(root, "long", "ended"), field(root, "long", "report")), ("done".into(), "2026-10-09T06:00:00Z".into(), "done report".into()));
    ingest(root, &row_ending("long", "not done", "2026-10-09T03:00:00Z"));
    assert_eq!(field(root, "long", "outcome"), "done", "an earlier end never replaces a later one");
    let ids = threads(root, "run-ids").args(["--settled", "--ended"]).output().unwrap();
    assert!(String::from_utf8(ids.stdout).unwrap().contains("long\t2026-10-09T06:00:00Z"));
}
