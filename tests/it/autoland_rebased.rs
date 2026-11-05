use north_core::foundation::shim;
use std::fs;
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
        shim::write_script(
            &gh,
            &format!(
                "case \"$1\" in api) printf '{autoland_status}\\n';; run) printf '[]\\n';; *) exit 1;; esac\n"
            ),
        );
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

