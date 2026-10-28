use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BRANCH: &str = "claude/land-rebased";

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &Path, file: &str, content: &str, subject: &str) -> String {
    fs::write(repo.join(file), content).unwrap();
    git(repo, &["add", file]);
    git(repo, &["commit", "-qm", subject]);
    git(repo, &["rev-parse", "HEAD"])
}

struct Scenario {
    _fixture: tempfile::TempDir,
    repo: PathBuf,
    paths: Vec<PathBuf>,
}

impl Scenario {
    fn new(autoland_status: &str) -> Self {
        let fixture = tempfile::tempdir().unwrap();
        let repo = fixture.path().join("repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(
            &repo,
            &["config", "user.name", "Autoland recorded scenario"],
        );
        git(&repo, &["config", "user.email", "autoland@example.invalid"]);
        let tools = fixture.path().join("tools");
        fs::create_dir(&tools).unwrap();
        let gh = tools.join("gh");
        fs::write(
            &gh,
            format!(
                "#!/bin/sh\ncase \"$1\" in api) printf '{autoland_status}\\n';; run) printf '[]\\n';; *) exit 1;; esac\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        let mut paths = vec![tools];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        Self {
            _fixture: fixture,
            repo,
            paths,
        }
    }

    fn publish(&self) {
        let origin = self.repo.parent().unwrap().join("origin.git");
        git(
            &self.repo,
            &["clone", "-q", "--bare", ".", origin.to_str().unwrap()],
        );
        git(
            &self.repo,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
    }

    fn wait(&self, sha: &str, timeout_seconds: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_north"))
            .current_dir(&self.repo)
            .env("PATH", std::env::join_paths(&self.paths).unwrap())
            .args([
                "autoland-wait",
                "--sha",
                sha,
                "--branch",
                BRANCH,
                "--destination",
                "main",
                "--timeout-seconds",
                timeout_seconds,
                "--poll-seconds",
                "1",
            ])
            .output()
            .unwrap()
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn rebased_autoland_reports_the_landed_head_when_the_original_has_no_status() {
    let scenario = Scenario::new("\\t\\t");
    let repo = &scenario.repo;
    commit(repo, "base", "base\n", "base");
    git(repo, &["switch", "-qc", "lane"]);
    let original = commit(repo, "change", "the lane change\n", "lane change");
    git(repo, &["switch", "-q", "main"]);
    commit(repo, "peer", "main moved\n", "peer landed first");
    git(repo, &["cherry-pick", &original]);
    let landed = git(repo, &["rev-parse", "HEAD"]);
    assert_ne!(original, landed);
    scenario.publish();
    git(repo, &["switch", "-q", "lane"]);
    let output = scenario.wait(&original, "0");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(stdout(&output), format!("success\tlanded {landed}\t\n"));
    let unlanded = commit(repo, "unlanded", "another change\n", "still waiting");
    let pending = scenario.wait(&unlanded, "0");
    assert_eq!(pending.status.code(), Some(2));
    assert!(stdout(&pending).starts_with("timeout\t"));
}

#[test]
fn a_patch_equivalent_tip_does_not_land_a_lane_whose_earlier_commit_is_absent() {
    let scenario = Scenario::new("\\t\\t");
    let repo = &scenario.repo;
    commit(repo, "base", "base\n", "base");
    git(repo, &["switch", "-qc", "lane"]);
    let earlier = commit(repo, "clip", "the clip fix\n", "fix the clip");
    let tip = commit(repo, "table", "strike table\n", "refresh the strike table");
    git(repo, &["switch", "-q", "main"]);
    git(repo, &["cherry-pick", &tip]);
    let lookalike = git(repo, &["rev-parse", "HEAD"]);
    assert_ne!(tip, lookalike);
    scenario.publish();
    git(repo, &["switch", "-q", "lane"]);
    let waiting = scenario.wait(&tip, "0");
    assert_eq!(waiting.status.code(), Some(2), "{}", stdout(&waiting));
    assert!(stdout(&waiting).starts_with("timeout\t"));
    git(repo, &["switch", "-q", "main"]);
    git(repo, &["cherry-pick", &earlier]);
    git(repo, &["push", "-q", "origin", "main"]);
    git(repo, &["switch", "-q", "lane"]);
    let landed = scenario.wait(&tip, "0");
    assert_eq!(landed.status.code(), Some(0), "{}", stdout(&landed));
    assert_eq!(stdout(&landed), format!("success\tlanded {lookalike}\t\n"));
}

struct ChangedContext {
    scenario: Scenario,
    lane_tip: String,
    landed: String,
}

fn changed_context_scenario() -> ChangedContext {
    let scenario = Scenario::new("pending\\twaiting for the deny-list attestation\\t");
    let repo = &scenario.repo;
    commit(repo, "file", "a\nb\nc\nd\ne\n", "base");
    git(repo, &["switch", "-qc", "lane"]);
    let lane_tip = commit(repo, "file", "a\nb\nC\nd\ne\n", "change c");
    git(repo, &["switch", "-q", "main"]);
    commit(repo, "file", "a\nB\nc\nd\ne\n", "peer edits the context");
    let landed = commit(repo, "file", "a\nB\nC\nd\ne\n", "change c");
    git(repo, &["switch", "-q", "lane"]);
    assert_ne!(lane_tip, landed);
    ChangedContext {
        scenario,
        lane_tip,
        landed,
    }
}

#[test]
fn a_deleted_lane_branch_with_the_lane_subject_on_main_ends_a_pending_wait() {
    let ChangedContext {
        scenario,
        lane_tip,
        landed,
    } = changed_context_scenario();
    scenario.publish();
    let output = scenario.wait(&lane_tip, "2");
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert_eq!(stdout(&output), format!("success\tlanded {landed}\t\n"));
}

#[test]
fn a_lane_branch_still_on_origin_keeps_a_pending_wait_until_timeout() {
    let ChangedContext {
        scenario, lane_tip, ..
    } = changed_context_scenario();
    scenario.publish();
    git(
        &scenario.repo,
        &["push", "-q", "origin", &format!("lane:{BRANCH}")],
    );
    let output = scenario.wait(&lane_tip, "2");
    assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));
    assert!(stdout(&output).starts_with("timeout\t"));
}

#[test]
fn a_deleted_lane_branch_without_the_lane_subject_on_main_keeps_waiting() {
    let scenario = Scenario::new("pending\\twaiting for the deny-list attestation\\t");
    let repo = &scenario.repo;
    commit(repo, "base", "base\n", "base");
    git(repo, &["switch", "-qc", "lane"]);
    let tip = commit(repo, "change", "the lane change\n", "lane change");
    git(repo, &["switch", "-q", "main"]);
    commit(repo, "peer", "main moved\n", "an unrelated subject");
    scenario.publish();
    git(repo, &["switch", "-q", "lane"]);
    let output = scenario.wait(&tip, "2");
    assert_eq!(output.status.code(), Some(2), "{}", stdout(&output));
    assert!(stdout(&output).starts_with("timeout\t"));
}
