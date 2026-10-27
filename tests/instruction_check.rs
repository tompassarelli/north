use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git").arg("-C").arg(repo).args(args).status().unwrap();
    assert!(status.success(), "git {args:?}");
}

fn lines(count: usize) -> String {
    "rule\n".repeat(count)
}

fn save(repo: &Path) -> String {
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "fixture"]);
    let output = Command::new("git").arg("-C").arg(repo).args(["rev-parse", "HEAD"]).output().unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn check(repo: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_north")).arg("agent-instruction-check").arg("--repo").arg(repo).args(extra).output().unwrap()
}

#[test]
fn instruction_files_are_held_to_one_hundred_lines_except_catalog_baselines() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path();
    git(repo, &["init", "-q", "-b", "main"]);
    git(repo, &["config", "user.name", "instruction-test"]);
    git(repo, &["config", "user.email", "instruction-test@example.invalid"]);
    fs::create_dir_all(repo.join("nested")).unwrap();

    fs::write(repo.join("AGENTS.md"), lines(100)).unwrap();
    fs::write(repo.join("nested/AGENTS.md"), lines(100)).unwrap();
    let valid = save(repo);
    assert_eq!(check(repo, &[]).status.code(), Some(0), "root and nested accept 100 lines");

    fs::write(repo.join("AGENTS.md"), lines(101)).unwrap();
    save(repo);
    let refused = check(repo, &[]);
    assert_eq!(refused.status.code(), Some(1), "root refuses 101 lines");
    assert!(String::from_utf8_lossy(&refused.stderr).contains("AGENTS.md@"));
    assert_eq!(check(repo, &["--ref", &valid]).status.code(), Some(0), "captured ref ignores newer oversized file");

    fs::write(repo.join("AGENTS.md"), lines(100)).unwrap();
    fs::write(repo.join("nested/AGENTS.md"), lines(101)).unwrap();
    save(repo);
    assert_eq!(check(repo, &[]).status.code(), Some(1), "nested refuses 101 lines");

    fs::write(repo.join("nested/AGENTS.md"), lines(100)).unwrap();
    fs::create_dir_all(repo.join("dotfiles/agents")).unwrap();
    fs::write(
        repo.join("dotfiles/agents/catalog-config.json"),
        r#"{"baselines":[{"owner":{"repo":"nixos-config","path":"dotfiles/agents/AGENTS.md"},"targets":["shared","codex"]}]}"#,
    )
    .unwrap();
    fs::write(repo.join("dotfiles/agents/AGENTS.md"), lines(197)).unwrap();
    save(repo);
    assert_eq!(check(repo, &[]).status.code(), Some(0), "catalog baseline is exempt from the repository limit");

    assert_eq!(check(repo, &["--ref", "no-such-ref"]).status.code(), Some(2), "an unresolvable ref cannot be checked");
}

#[test]
fn safe_push_refuses_a_branch_or_tag_whose_instruction_files_are_oversized() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let scratch = tempfile::tempdir().unwrap();
    let (repo, bin, remote) = (scratch.path().join("repo"), scratch.path().join("bin"), scratch.path().join("remote"));
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&repo).unwrap();
    fs::write(bin.join("gitleaks"), "#!/usr/bin/env bash\nexit 0\n").unwrap();
    fs::set_permissions(bin.join("gitleaks"), fs::Permissions::from_mode(0o755)).unwrap();
    symlink(env!("CARGO_BIN_EXE_north"), bin.join("north")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "instruction-test"]);
    git(&repo, &["config", "user.email", "instruction-test@example.invalid"]);
    git(scratch.path(), &["init", "--bare", "-q", "-b", "main", remote.to_str().unwrap()]);
    git(&repo, &["remote", "add", "origin", remote.to_str().unwrap()]);
    fs::write(repo.join("AGENTS.md"), lines(101)).unwrap();
    save(&repo);

    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let safe_push = |extra: &[&str]| {
        Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/safe-push"))
            .current_dir(&repo)
            .env("PATH", &path)
            .arg("--dry-run")
            .args(extra)
            .output()
            .unwrap()
    };
    let branch = safe_push(&[]);
    assert_eq!(branch.status.code(), Some(1), "branch publication refuses oversized instructions: {}", String::from_utf8_lossy(&branch.stderr));
    git(&repo, &["tag", "-a", "release", "-m", "release"]);
    let tag = safe_push(&["--tag", "release"]);
    assert_eq!(tag.status.code(), Some(1), "tag publication checks its oversized target: {}", String::from_utf8_lossy(&tag.stderr));
}
