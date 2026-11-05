use std::path::Path;
use std::time::Duration;

use super::ledger::{HEADER, Verdict, suggested_threshold};
use super::measure::{Cargo, median};
use super::{Options, config, measure_repo, timings};
use crate::platform::rolling_issue;

const TIMINGS: &str = r#"<script>
const UNIT_DATA = [
  {"i": 0, "name": "syn", "target": "", "start": 0.0, "duration": 4.0, "unblocked_units": [], "unblocked_rmeta_units": [2], "sections": null},
  {"i": 1, "name": "libc", "target": "", "start": 0.0, "duration": 1.5, "unblocked_units": [2], "unblocked_rmeta_units": [], "sections": null},
  {"i": 2, "name": "app", "target": "", "start": 3.5, "duration": 10.0, "unblocked_units": [3], "unblocked_rmeta_units": [], "sections": null},
  {"i": 3, "name": "app", "target": " bin \"app\"", "start": 13.5, "duration": 2.0, "unblocked_units": [], "unblocked_rmeta_units": [], "sections": null}
];
const CONCURRENCY_DATA = [];
</script>"#;

const STRUCTURE: &str = "[thresholds]\nfile_lines = 500\n\n[[build_time]]\nworkspace = \".\"\nleaf = \"src/leaf.rs\"\nhub = \"src/hub.rs\"\ntest_no_run_seconds = 30\n";

struct FakeCargo {
    test_seconds: u64,
    calls: Vec<String>,
}

impl Cargo for FakeCargo {
    fn run(&mut self, _workspace: &Path, target: &Path, args: &[&str]) -> Result<Duration, String> {
        self.calls.push(args.join(" "));
        if args.contains(&"--timings") {
            std::fs::create_dir_all(target.join("cargo-timings")).map_err(|error| error.to_string())?;
            std::fs::write(target.join("cargo-timings/cargo-timing.html"), TIMINGS).map_err(|error| error.to_string())?;
            return Ok(Duration::from_secs(90));
        }
        Ok(Duration::from_secs(if args[0] == "test" { self.test_seconds } else { 4 }))
    }
}

fn fixture() -> tempfile::TempDir {
    let repo = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(repo.path().join("src")).expect("src");
    std::fs::write(repo.path().join("structure.toml"), STRUCTURE).expect("structure");
    std::fs::write(repo.path().join("src/leaf.rs"), "pub fn leaf() {}\n").expect("leaf");
    std::fs::write(repo.path().join("src/hub.rs"), "pub fn hub() {}").expect("hub");
    repo
}

fn measured(test_seconds: u64) -> (u8, String, FakeCargo, tempfile::TempDir) {
    let repo = fixture();
    let ledger = repo.path().join("state/build-times.tsv");
    let options = Options { repo: repo.path().to_path_buf(), samples: 3, ..Options::default() };
    let mut cargo = FakeCargo { test_seconds, calls: Vec::new() };
    let code = measure_repo(&options, &mut cargo, &ledger, "gex45", None).expect("measured");
    let text = std::fs::read_to_string(&ledger).expect("ledger");
    (code, text, cargo, repo)
}

#[test]
fn leaf_test_no_run_within_threshold_passes_and_records_a_row() {
    let (code, ledger, cargo, repo) = measured(12);
    assert_eq!(code, 0);
    let lines: Vec<&str> = ledger.lines().collect();
    assert_eq!(lines[0], HEADER);
    let row: Vec<&str> = lines[1].split('\t').collect();
    assert_eq!((row[1], row[6], row[7], row[8], row[9], row[12], row[13]), ("gex45", "3", "90.0", "4.0", "12.0", "30", "within"));
    assert_eq!(cargo.calls[0], "build --timings");
    assert_eq!(cargo.calls.iter().filter(|call| call.as_str() == "test --no-run").count(), 8);
    assert_eq!(std::fs::read_to_string(repo.path().join("src/leaf.rs")).expect("leaf"), "pub fn leaf() {}\n");
    assert_eq!(std::fs::read_to_string(repo.path().join("src/hub.rs")).expect("hub"), "pub fn hub() {}");
    assert!(std::fs::read_dir(repo.path().join("target")).expect("target").next().is_none());
}

#[test]
fn leaf_test_no_run_over_threshold_exits_non_zero_and_appends() {
    let (code, ledger, _, repo) = measured(31);
    assert_eq!(code, 1);
    assert!(ledger.lines().nth(1).expect("row").contains("\t30\tover\t"));
    let ledger_path = repo.path().join("state/build-times.tsv");
    let options = Options { repo: repo.path().to_path_buf(), samples: 1, ..Options::default() };
    measure_repo(&options, &mut FakeCargo { test_seconds: 23, calls: Vec::new() }, &ledger_path, "gex45", None).expect("measured");
    let again = std::fs::read_to_string(&ledger_path).expect("ledger");
    assert_eq!(again.lines().count(), 3);
    assert!(again.lines().nth(2).expect("row").contains("\t30\tnear\t"));
}

#[test]
fn unconfigured_workspace_needs_leaf_and_hub() {
    let repo = fixture();
    let options = Options { repo: repo.path().to_path_buf(), workspace: Some("relay".into()), samples: 1, ..Options::default() };
    let error = measure_repo(&options, &mut FakeCargo { test_seconds: 1, calls: Vec::new() }, &repo.path().join("l.tsv"), "h", None).expect_err("unconfigured");
    assert!(error.contains("pass --leaf and --hub"), "{error}");
}

#[test]
fn critical_path_follows_the_latest_unblocking_unit() {
    assert_eq!(timings::critical_path(TIMINGS).expect("path"), "15.5s: syn 4.0s > app 10.0s > app (bin \"app\") 2.0s");
}

#[test]
fn config_rejects_unknown_keys_and_reads_thresholds() {
    assert_eq!(config::parse(STRUCTURE).expect("parsed")[0].threshold_seconds, Some(30.0));
    assert!(config::parse("[[build_time]]\nworkspace = \".\"\nleaf = \"a\"\nhub = \"b\"\nlimit = 3\n").is_err());
    assert!(config::parse("[thresholds]\nfile_lines = 1\n").expect("none").is_empty());
}

#[test]
fn verdicts_and_suggested_thresholds() {
    assert_eq!(Verdict::of(10.0, Some(30.0)), Verdict::Within);
    assert_eq!(Verdict::of(22.5, Some(30.0)), Verdict::Near);
    assert_eq!(Verdict::of(30.5, Some(30.0)), Verdict::Over);
    assert_eq!(Verdict::of(30.5, None), Verdict::Unset);
    assert_eq!(suggested_threshold(9.0), 30.0);
    assert_eq!(suggested_threshold(21.2), 43.0);
    assert_eq!(median(vec![5.0, 1.0, 3.0]), 3.0);
    assert_eq!(median(vec![4.0, 1.0]), 2.5);
}

#[derive(Default)]
struct Issues {
    open: Option<rolling_issue::Issue>,
    created: Vec<String>,
    edited: Vec<String>,
    closed: Vec<u64>,
}

impl rolling_issue::Tracker for Issues {
    fn open(&self, title: &str) -> Result<Option<rolling_issue::Issue>, String> {
        assert!(title.starts_with("Split a slow Rust workspace in "), "{title}");
        Ok(self.open.as_ref().map(|issue| rolling_issue::Issue { number: issue.number, body: issue.body.clone() }))
    }

    fn listed(&self, _: &str) -> Result<Vec<(u64, String)>, String> {
        Ok(Vec::new())
    }

    fn create(&mut self, _: &str, body: &str) -> Result<(), String> {
        self.created.push(body.to_owned());
        self.open = Some(rolling_issue::Issue { number: 41, body: body.to_owned() });
        Ok(())
    }

    fn edit(&mut self, number: u64, body: &str) -> Result<(), String> {
        assert_eq!(number, 41);
        self.edited.push(body.to_owned());
        self.open = Some(rolling_issue::Issue { number, body: body.to_owned() });
        Ok(())
    }

    fn close(&mut self, number: u64, _: &str) -> Result<(), String> {
        self.closed.push(number);
        self.open = None;
        Ok(())
    }
}

#[test]
fn a_workspace_over_its_threshold_files_one_rolling_split_box_that_later_runs_edit_and_close() {
    let repo = fixture();
    let ledger = repo.path().join("state/build-times.tsv");
    let options = Options { repo: repo.path().to_path_buf(), samples: 1, file: true, ..Options::default() };
    let mut issues = Issues::default();
    let run = |test_seconds: u64, issues: &mut Issues| measure_repo(&options, &mut FakeCargo { test_seconds, calls: Vec::new() }, &ledger, "gex45", Some(issues)).expect("measured");

    assert_eq!(run(31, &mut issues), 1);
    assert_eq!(issues.created.len(), 1);
    let filed = &issues.created[0];
    assert_eq!(filed.matches("- [ ] Split `.`").count(), 1, "{filed}");
    assert!(filed.contains("took 31.0s against a threshold of 30s (over)"), "{filed}");
    assert!(filed.contains("docs/rationale/rust-build-times.md"), "{filed}");

    assert_eq!(run(33, &mut issues), 1);
    assert_eq!((issues.created.len(), issues.edited.len()), (1, 1));
    assert!(issues.edited[0].contains("took 33.0s against a threshold of 30s (over)"));

    assert_eq!(run(33, &mut issues), 1);
    assert_eq!((issues.created.len(), issues.edited.len()), (1, 1), "an unchanged measurement leaves the issue alone");

    assert_eq!(run(23, &mut issues), 0);
    assert!(issues.edited[1].contains("(near)"));

    assert_eq!(run(12, &mut issues), 0);
    assert_eq!((issues.created.len(), issues.closed.as_slice()), (1, [41].as_slice()));
}

#[test]
fn filing_needs_every_configured_workspace() {
    let arguments: Vec<String> = ["--repo", ".", "--workspace", ".", "--leaf", "a", "--hub", "b", "--file"].map(str::to_owned).to_vec();
    assert!(super::parse(&arguments).is_err());
    let rev: Vec<String> = ["--repo", ".", "--rev", "origin/main", "--file"].map(str::to_owned).to_vec();
    let parsed = super::parse(&rev).expect("parsed");
    assert_eq!((parsed.rev.as_deref(), parsed.file), (Some("origin/main"), true));
}
