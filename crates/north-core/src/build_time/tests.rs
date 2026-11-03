use std::path::Path;
use std::time::Duration;

use super::ledger::{HEADER, Verdict, suggested_threshold};
use super::measure::{Cargo, median};
use super::{Options, config, measure_repo, timings};

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
    let code = measure_repo(&options, &mut cargo, &ledger, "gex45").expect("measured");
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
    measure_repo(&options, &mut FakeCargo { test_seconds: 23, calls: Vec::new() }, &ledger_path, "gex45").expect("measured");
    let again = std::fs::read_to_string(&ledger_path).expect("ledger");
    assert_eq!(again.lines().count(), 3);
    assert!(again.lines().nth(2).expect("row").contains("\t30\tnear\t"));
}

#[test]
fn unconfigured_workspace_needs_leaf_and_hub() {
    let repo = fixture();
    let options = Options { repo: repo.path().to_path_buf(), workspace: Some("relay".into()), samples: 1, ..Options::default() };
    let error = measure_repo(&options, &mut FakeCargo { test_seconds: 1, calls: Vec::new() }, &repo.path().join("l.tsv"), "h").expect_err("unconfigured");
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
