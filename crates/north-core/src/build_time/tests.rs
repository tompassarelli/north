use std::path::Path;
use std::time::Duration;

use super::ledger::{HEADER, Row, Verdict};
use super::measure::{Cargo, Times, median};
use super::threshold::{LEAF_TEST_NO_RUN_SECONDS_MAX, limit};
use super::{Options, config, issue, measure_repo, timings};
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
    fixture_with(STRUCTURE)
}

fn fixture_with(structure: &str) -> tempfile::TempDir {
    let repo = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(repo.path().join("src")).expect("src");
    std::fs::write(repo.path().join("structure.toml"), structure).expect("structure");
    std::fs::write(repo.path().join("src/leaf.rs"), "pub fn leaf() {}\n").expect("leaf");
    std::fs::write(repo.path().join("src/hub.rs"), "pub fn hub() {}").expect("hub");
    repo
}

fn measured(test_seconds: u64) -> (u8, String, FakeCargo, tempfile::TempDir) {
    measured_in(STRUCTURE, test_seconds)
}

fn measured_in(structure: &str, test_seconds: u64) -> (u8, String, FakeCargo, tempfile::TempDir) {
    let repo = fixture_with(structure);
    let ledger = repo.path().join("state/build-times.tsv");
    let options = Options { repo: repo.path().to_path_buf(), samples: 3, ..Options::default() };
    let mut cargo = FakeCargo { test_seconds, calls: Vec::new() };
    let code = measure_repo(&options, &mut cargo, &ledger, "gex45", None, None).expect("measured");
    let text = std::fs::read_to_string(&ledger).expect("ledger");
    (code, text, cargo, repo)
}

#[test]
fn leaf_test_no_run_within_threshold_passes_and_records_a_row() {
    let (code, ledger, cargo, repo) = measured(6);
    assert_eq!(code, 0);
    let lines: Vec<&str> = ledger.lines().collect();
    assert_eq!(lines[0], HEADER);
    let row: Vec<&str> = lines[1].split('\t').collect();
    assert_eq!((row[1], row[6], row[7], row[8], row[9], row[12], row[13]), ("gex45", "3", "90.0", "4.0", "6.0", "10", "within"));
    assert_eq!(cargo.calls[0], "build --timings");
    assert_eq!(cargo.calls.iter().filter(|call| call.as_str() == "test --no-run").count(), 8);
    assert_eq!(std::fs::read_to_string(repo.path().join("src/leaf.rs")).expect("leaf"), "pub fn leaf() {}\n");
    assert_eq!(std::fs::read_to_string(repo.path().join("src/hub.rs")).expect("hub"), "pub fn hub() {}");
    assert!(std::fs::read_dir(repo.path().join("target")).expect("target").next().is_none());
}

#[test]
fn leaf_test_no_run_over_threshold_exits_non_zero_and_appends() {
    let (code, ledger, _, repo) = measured(11);
    assert_eq!(code, 1);
    assert!(ledger.lines().nth(1).expect("row").contains("\t10\tover\t"));
    let ledger_path = repo.path().join("state/build-times.tsv");
    let options = Options { repo: repo.path().to_path_buf(), samples: 1, ..Options::default() };
    measure_repo(&options, &mut FakeCargo { test_seconds: 8, calls: Vec::new() }, &ledger_path, "gex45", None, None).expect("measured");
    let again = std::fs::read_to_string(&ledger_path).expect("ledger");
    assert_eq!(again.lines().count(), 3);
    assert!(again.lines().nth(2).expect("row").contains("\t10\tnear\t"));
}

#[test]
fn build_time_above_the_ten_second_cap_is_over_whatever_structure_toml_allows() {
    let (code, ledger, _, _) = measured(29);
    assert_eq!(code, 1);
    assert!(ledger.lines().nth(1).expect("row").contains("\t10\tover\t"), "{ledger}");
    let unconfigured = "[[build_time]]\nworkspace = \".\"\nleaf = \"src/leaf.rs\"\nhub = \"src/hub.rs\"\n";
    let (code, ledger, _, _) = measured_in(unconfigured, 11);
    assert_eq!(code, 1);
    assert!(ledger.lines().nth(1).expect("row").contains("\t10\tover\t"), "{ledger}");
    let (code, ledger, _, _) = measured_in(unconfigured, 6);
    assert_eq!(code, 0);
    assert!(ledger.lines().nth(1).expect("row").contains("\t10\twithin\t"), "{ledger}");
}

#[test]
fn build_time_structure_toml_may_only_lower_the_cap() {
    let lowered = STRUCTURE.replace("test_no_run_seconds = 30", "test_no_run_seconds = 6");
    let (code, ledger, _, _) = measured_in(&lowered, 7);
    assert_eq!(code, 1);
    assert!(ledger.lines().nth(1).expect("row").contains("\t6\tover\t"), "{ledger}");
    assert_eq!(LEAF_TEST_NO_RUN_SECONDS_MAX, 10.0);
    let (cap, warning) = limit(".", Some(88.0));
    assert_eq!(cap, 10.0);
    assert!(warning.expect("warned").contains("test_no_run_seconds = 88 is above the 10 s cap, which it may only lower; using 10 s"));
    assert_eq!(limit(".", Some(10.0)), (10.0, None));
    assert_eq!(limit(".", Some(6.0)), (6.0, None));
    assert_eq!(limit(".", None), (10.0, None));
}

#[test]
fn unconfigured_workspace_needs_leaf_and_hub() {
    let repo = fixture();
    let options = Options { repo: repo.path().to_path_buf(), workspace: Some("relay".into()), samples: 1, ..Options::default() };
    let error = measure_repo(&options, &mut FakeCargo { test_seconds: 1, calls: Vec::new() }, &repo.path().join("l.tsv"), "h", None, None).expect_err("unconfigured");
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
fn verdicts_and_the_cap() {
    assert_eq!(Verdict::of(10.0, 30.0), Verdict::Within);
    assert_eq!(Verdict::of(22.5, 30.0), Verdict::Near);
    assert_eq!(Verdict::of(30.5, 30.0), Verdict::Over);
    assert_eq!(Verdict::of(7.4, LEAF_TEST_NO_RUN_SECONDS_MAX), Verdict::Within);
    assert_eq!(Verdict::of(7.5, LEAF_TEST_NO_RUN_SECONDS_MAX), Verdict::Near);
    assert_eq!(Verdict::of(10.0, LEAF_TEST_NO_RUN_SECONDS_MAX), Verdict::Near);
    assert_eq!(Verdict::of(10.1, LEAF_TEST_NO_RUN_SECONDS_MAX), Verdict::Over);
    assert_eq!([Verdict::Within, Verdict::Near, Verdict::Over].map(Verdict::word), ["within", "near", "over"]);
    assert_eq!(median(vec![5.0, 1.0, 3.0]), 3.0);
    assert_eq!(median(vec![4.0, 1.0]), 2.5);
}

const ISSUE_259: &str = "Category: feature\n\n## Split north-core\n- [x] Cut ops back-edges\n- [ ] Crate-size gate\n\n## Notes\n- [ ] Leaf-edit cargo test --no-run in elsewhere is 99.0 s, kept\n";

#[derive(Default)]
struct Issues {
    body: String,
    opened: Vec<String>,
    created: Vec<String>,
    edited: Vec<String>,
    closed: Vec<u64>,
}

impl rolling_issue::Tracker for Issues {
    fn open(&self, title: &str) -> Result<Option<rolling_issue::Issue>, String> {
        Err(format!("opened a rolling issue {title}"))
    }

    fn body(&self, number: u64) -> Result<String, String> {
        assert_eq!(number, 259);
        Ok(self.body.clone())
    }

    fn listed(&self, _: &str) -> Result<Vec<(u64, String)>, String> {
        Ok(Vec::new())
    }

    fn create(&mut self, title: &str, body: &str) -> Result<(), String> {
        self.opened.push(title.to_owned());
        self.created.push(body.to_owned());
        Ok(())
    }

    fn edit(&mut self, number: u64, body: &str) -> Result<(), String> {
        assert_eq!(number, 259);
        self.edited.push(body.to_owned());
        self.body = body.to_owned();
        Ok(())
    }

    fn close(&mut self, number: u64, _: &str) -> Result<(), String> {
        self.closed.push(number);
        Ok(())
    }
}

fn section(body: &str) -> String {
    let start = body.find(issue::SECTION).expect("section");
    let rest = &body[start..];
    rest[..rest[3..].find("\n## ").map_or(rest.len(), |end| end + 4)].to_owned()
}

#[test]
fn build_time_over_the_cap_ticks_one_box_per_workspace_on_split_north_core_and_opens_no_rolling_issue() {
    let repo = fixture();
    let ledger = repo.path().join("state/build-times.tsv");
    let options = Options { repo: repo.path().to_path_buf(), samples: 1, file: true, ..Options::default() };
    let mut issues = Issues { body: ISSUE_259.to_owned(), ..Issues::default() };
    let run = |test_seconds: u64, issues: &mut Issues| measure_repo(&options, &mut FakeCargo { test_seconds, calls: Vec::new() }, &ledger, "gex45", None, Some(issues)).expect("measured");
    let key = "Leaf-edit cargo test --no-run in ";

    assert_eq!(run(44, &mut issues), 1);
    assert_eq!(issues.edited.len(), 1);
    let filed = section(&issues.body);
    assert_eq!(filed.matches(key).count(), 1, "{filed}");
    assert!(filed.contains(" is 44.0 s, over the 10 s cap: split the crate holding src/leaf.rs (measured "), "{filed}");
    assert!(filed.contains(" on gex45 at "), "{filed}");
    assert!(filed.starts_with("## Split north-core\n- [x] Cut ops back-edges\n- [ ] Crate-size gate\n- [ ] Leaf-edit"), "{filed}");
    assert!(issues.body.ends_with("## Notes\n- [ ] Leaf-edit cargo test --no-run in elsewhere is 99.0 s, kept\n"), "{}", issues.body);

    assert_eq!(run(33, &mut issues), 1);
    assert_eq!(issues.edited.len(), 2);
    let filed = section(&issues.body);
    assert_eq!(filed.matches(key).count(), 1, "an existing box's numbers are edited, not duplicated: {filed}");
    assert!(filed.contains(" is 33.0 s, over the 10 s cap"), "{filed}");

    assert_eq!(run(33, &mut issues), 1);
    assert_eq!(issues.edited.len(), 2, "an unchanged measurement leaves the issue alone");

    assert_eq!(run(8, &mut issues), 0);
    assert!(section(&issues.body).contains(" is 8.0 s, near the 10 s cap"), "{}", issues.body);

    assert_eq!(run(6, &mut issues), 0);
    let filed = section(&issues.body);
    assert_eq!(filed.matches(key).count(), 1, "{filed}");
    assert!(filed.contains("- [x] Leaf-edit cargo test --no-run in "), "{filed}");
    assert!(filed.contains(" is 6.0 s, within the 10 s cap (measured "), "{filed}");
    assert!(issues.opened.is_empty() && issues.created.is_empty() && issues.closed.is_empty(), "no rolling issue is opened, created or closed");
}

#[test]
fn build_time_within_the_cap_adds_no_box_and_a_missing_section_is_created() {
    let repo = fixture();
    let ledger = repo.path().join("state/build-times.tsv");
    let options = Options { repo: repo.path().to_path_buf(), samples: 1, file: true, ..Options::default() };
    let mut issues = Issues { body: "Category: feature\n".to_owned(), ..Issues::default() };
    let run = |test_seconds: u64, issues: &mut Issues| measure_repo(&options, &mut FakeCargo { test_seconds, calls: Vec::new() }, &ledger, "gex45", None, Some(issues)).expect("measured");

    assert_eq!(run(6, &mut issues), 0);
    assert!(issues.edited.is_empty());
    assert_eq!(run(12, &mut issues), 1);
    assert!(issues.body.starts_with("Category: feature\n\n## Split north-core\n- [ ] Leaf-edit cargo test --no-run in "), "{}", issues.body);
    assert!(issues.created.is_empty());
}

#[test]
fn build_time_files_one_box_per_workspace() {
    let slow = Times { leaf_test: 43.8, ..Times::default() };
    let slower = Times { leaf_test: 11.0, ..Times::default() };
    let split = |workspace: &str, times: &Times| {
        let row = Row { recorded_at: "2026-11-05T04:00:00Z", host: "gex45", repo: "north", workspace, sha: "abc", rustc: "1.96.1", samples: 1, times, threshold: 10.0, verdict: Verdict::of(times.leaf_test, 10.0), contention: None };
        issue::split_box(&row, "crates/x/src/leaf.rs").expect("box")
    };
    let boxes = [split(".", &slow), split("tools/firn", &slower)];
    let body = issue::edited("## Split north-core\n", &boxes);
    assert_eq!(
        body,
        "## Split north-core\n- [ ] Leaf-edit cargo test --no-run in north is 43.8 s, over the 10 s cap: split the crate holding crates/x/src/leaf.rs (measured 2026-11-05 on gex45 at abc)\n- [ ] Leaf-edit cargo test --no-run in north/tools/firn is 11.0 s, over the 10 s cap: split the crate holding crates/x/src/leaf.rs (measured 2026-11-05 on gex45 at abc)\n"
    );
    assert_eq!(issue::edited(&body, &boxes), body);
}

#[test]
fn filing_needs_every_configured_workspace() {
    let arguments: Vec<String> = ["--repo", ".", "--workspace", ".", "--leaf", "a", "--hub", "b", "--file"].map(str::to_owned).to_vec();
    assert!(super::parse(&arguments).is_err());
    let rev: Vec<String> = ["--repo", ".", "--rev", "origin/main", "--file"].map(str::to_owned).to_vec();
    let parsed = super::parse(&rev).expect("parsed");
    assert_eq!((parsed.rev.as_deref(), parsed.file), (Some("origin/main"), true));
}

#[test]
fn a_contended_run_over_the_cap_files_nothing_and_leaves_an_open_box_alone() {
    let structure = format!("{STRUCTURE}contention_ceiling = 0.5\n");
    let repo = fixture_with(&structure);
    let ledger = repo.path().join("state/build-times.tsv");
    let options = Options { repo: repo.path().to_path_buf(), samples: 1, file: true, ..Options::default() };
    let mut issues = Issues { body: ISSUE_259.to_owned(), ..Issues::default() };
    let run = |test_seconds: u64, contention: f64, issues: &mut Issues| measure_repo(&options, &mut FakeCargo { test_seconds, calls: Vec::new() }, &ledger, "gex45", Some(contention), Some(issues)).expect("measured");

    assert_eq!(run(202, 1.25, &mut issues), 0);
    assert!(issues.edited.is_empty(), "{}", issues.body);
    let text = std::fs::read_to_string(&ledger).expect("ledger");
    let fields: Vec<&str> = text.lines().nth(1).expect("row").split('\t').collect();
    assert_eq!((fields[13], fields[15]), ("contended", "1.25"));

    assert_eq!(run(202, 0.25, &mut issues), 1);
    assert_eq!(issues.edited.len(), 1);

    assert_eq!(run(6, 1.25, &mut issues), 0);
    assert_eq!(issues.edited.len(), 1, "a contended run does not tick the open box either");

    assert_eq!(run(6, 0.25, &mut issues), 0);
    assert_eq!(issues.edited.len(), 2);
}

#[test]
fn contention_ceiling_is_read_from_structure_toml_and_judges_only_above_it() {
    let with = format!("{STRUCTURE}contention_ceiling = 0.5\n");
    assert_eq!(config::parse(&with).expect("parsed")[0].contention_ceiling, Some(0.5));
    assert_eq!(config::parse(STRUCTURE).expect("parsed")[0].contention_ceiling, None);
    assert_eq!(Verdict::judge(202.1, 10.0, Some(1.25), Some(0.5)), Verdict::Contended);
    assert_eq!(Verdict::judge(202.1, 10.0, Some(0.5), Some(0.5)), Verdict::Over);
    assert_eq!(Verdict::judge(202.1, 10.0, None, Some(0.5)), Verdict::Over);
    assert_eq!(Verdict::judge(202.1, 10.0, Some(9.0), None), Verdict::Over);
}
