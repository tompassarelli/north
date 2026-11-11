use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::foundation::step::Step;

use super::route;
use crate::request::{self, Plan};
use crate::shell::processes::{Child, Finished, Live, Processes};
use crate::shell::settings::Settings;
use crate::shell::{Shell, files, git, github, origin};

fn git_in(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

struct ChecksPass;

impl Processes for ChecksPass {
    fn on_path(&self, program: &str) -> bool {
        Live.on_path(program)
    }
    fn current_exe(&self) -> PathBuf {
        PathBuf::from("north")
    }
    fn run_with_input(&self, program: &Path, directory: &Path, arguments: &[&str], input: &str) -> std::io::Result<Step> {
        Live.run_with_input(program, directory, arguments, input)
    }
    fn run_with_outcome(&self, _child: &Child) -> Finished {
        Finished { step: Ok(()), outcome: String::new() }
    }
    fn succeeds(&self, _program: &Path, _directory: &Path, _arguments: &[&str]) -> Option<String> {
        None
    }
    fn exit_code(&self, _program: &str, _arguments: &[&str]) -> Option<i32> {
        None
    }
    fn spawn_detached(&self, program: &Path, arguments: &[&str]) {
        Live.spawn_detached(program, arguments);
    }
    fn capture_output(&self, run: &mut dyn FnMut()) -> String {
        Live.capture_output(run)
    }
}

fn shell(lane: &Path, state: &Path) -> Shell {
    let settings = Settings {
        state_home: state.to_path_buf(),
        south: state.join("south"),
        mirrors: state.join("mirrors"),
        safe_push: "safe-push".to_owned(),
        fleet_session: String::new(),
        host: "fixture".to_owned(),
        pid: std::process::id(),
        output_claimed: String::new(),
        working_directory: lane.to_path_buf(),
        watcher: None,
        raw_exit: true,
        land_offload: false,
    };
    Shell {
        git: Box::new(git::Live::new(Path::new("."))),
        origin: Box::new(origin::Live::new(Path::new("."))),
        github: Box::new(github::Live),
        files: Box::new(files::Live::new(&settings.state_home)),
        processes: Box::new(ChecksPass),
        settings,
    }
}

#[test]
fn root_commit_lands_on_an_empty_origin() {
    let scratch = tempfile::tempdir().unwrap();
    let (origin, main, lane) = (scratch.path().join("origin.git"), scratch.path().join("main"), scratch.path().join("lane"));
    git_in(scratch.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    git_in(scratch.path(), &["init", "-q", "-b", "main", main.to_str().unwrap()]);
    git_in(&main, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git_in(&main, &["config", "user.email", "fixture@example.invalid"]);
    git_in(&main, &["worktree", "add", "-q", "--orphan", "-b", "init", lane.to_str().unwrap()]);
    std::fs::write(lane.join("README.md"), "a new project\n").unwrap();
    git_in(&lane, &["add", "-A"]);
    git_in(&lane, &["commit", "-qm", "Start the project"]);
    let head = git_in(&lane, &["rev-parse", "HEAD"]);

    std::env::set_current_dir(&lane).unwrap();
    let arguments = ["--to".to_owned(), "main".to_owned()];
    let Ok(Plan::Push(request)) = request::arguments(&arguments) else { panic!("arguments") };
    let code = route::land(&arguments, &request, "main", &shell(&lane, &scratch.path().join("state")));

    assert_eq!(code, 0);
    assert_eq!(git_in(&origin, &["rev-parse", "refs/heads/main"]), head);
}

#[test]
fn root_commit_scans_take_the_empty_tree_as_base() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    git_in(scratch.path(), &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
    std::fs::write(repo.join("README.md"), "a new project\n").unwrap();
    git_in(&repo, &["add", "-A"]);
    git_in(&repo, &["commit", "-qm", "Start the project"]);
    let head = git_in(&repo, &["rev-parse", "HEAD"]);

    std::env::set_current_dir(&repo).unwrap();
    for (scan, range) in [("dep-gate", format!("..{head}")), ("policy-sweep", head.clone()), ("docs-owners", head.clone()), ("doc-links", head)] {
        assert!(crate::gates::run::scan(&[scan, &range]).is_ok(), "{scan} refused the root commit's range {range}");
    }
}
