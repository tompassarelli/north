use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

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

#[test]
fn rebased_autoland_reports_the_landed_head_when_the_original_has_no_status() {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(
        &repo,
        &["config", "user.name", "Autoland recorded scenario"],
    );
    git(&repo, &["config", "user.email", "autoland@example.invalid"]);
    fs::write(repo.join("base"), "base\n").unwrap();
    git(&repo, &["add", "base"]);
    git(&repo, &["commit", "-qm", "base"]);
    git(&repo, &["switch", "-qc", "lane"]);
    fs::write(repo.join("change"), "the lane change\n").unwrap();
    git(&repo, &["add", "change"]);
    git(&repo, &["commit", "-qm", "lane change"]);
    let original = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["switch", "-q", "main"]);
    fs::write(repo.join("peer"), "main moved\n").unwrap();
    git(&repo, &["add", "peer"]);
    git(&repo, &["commit", "-qm", "peer landed first"]);
    git(&repo, &["cherry-pick", &original]);
    let landed = git(&repo, &["rev-parse", "HEAD"]);
    assert_ne!(original, landed);
    git(&repo, &["clone", "-q", "--bare", ".", "../origin.git"]);
    git(&repo, &["remote", "add", "origin", "../origin.git"]);
    git(&repo, &["switch", "-q", "lane"]);
    let tools = fixture.path().join("tools");
    fs::create_dir(&tools).unwrap();
    let gh = tools.join("gh");
    fs::write(&gh, "#!/bin/sh\ncase \"$1\" in api) printf '\\t\\t\\n';; run) printf '[]\\n';; *) exit 1;; esac\n").unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    let mut paths = vec![tools];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let wait = |sha: &str| {
        Command::new(env!("CARGO_BIN_EXE_north"))
            .current_dir(&repo)
            .env("PATH", std::env::join_paths(&paths).unwrap())
            .args([
                "autoland-wait",
                "--sha",
                sha,
                "--branch",
                "claude/land-rebased",
                "--destination",
                "main",
                "--timeout-seconds",
                "0",
            ])
            .output()
            .unwrap()
    };
    let output = wait(&original);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("success\tlanded {landed}\t\n")
    );
    fs::write(repo.join("unlanded"), "another change\n").unwrap();
    git(&repo, &["add", "unlanded"]);
    git(&repo, &["commit", "-qm", "still waiting"]);
    let pending = wait(&git(&repo, &["rev-parse", "HEAD"]));
    assert_eq!(pending.status.code(), Some(2));
    assert!(
        String::from_utf8(pending.stdout)
            .unwrap()
            .starts_with("timeout\t")
    );
}
