use std::fs::{self, File};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "init.defaultBranch=main", "-c", "protocol.file.allow=always"])
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn lane(main: &Path, worktrees: &Path, name: &str, file: &str) -> std::path::PathBuf {
    let path = worktrees.join(name);
    git(main, &["worktree", "add", "-q", "-b", name, path.to_str().unwrap(), "origin/main"]);
    fs::write(path.join(file), format!("{name} work\n")).unwrap();
    git(&path, &["add", file]);
    git(&path, &["commit", "-qm", &format!("{name} work")]);
    path
}

#[test]
fn a_lane_with_a_recorded_rejection_is_bundled_and_reaped_and_a_half_removed_one_is_archived() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let home = root.join("home");
    let container = home.join("code/proj");
    let (main, worktrees, archive) = (container.join("main"), container.join("worktrees"), home.join("archive/lanes"));
    fs::create_dir_all(&worktrees).unwrap();
    git(&root, &["init", "-q", "--bare", "origin.git"]);
    git(&root, &["init", "-q", main.to_str().unwrap()]);
    git(&main, &["remote", "add", "origin", root.join("origin.git").to_str().unwrap()]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "start"]);
    git(&main, &["push", "-q", "origin", "main"]);
    git(&main, &["fetch", "-q", "origin"]);
    fs::write(main.join(".git/info/exclude"), "target/\n").unwrap();

    let rejected = lane(&main, &worktrees, "rejected", "rejected.txt");
    fs::write(rejected.join("notes.txt"), "uncommitted\n").unwrap();
    let bystander = lane(&main, &worktrees, "bystander", "bystander.txt");
    let half = lane(&main, &worktrees, "half", "half.txt");
    fs::write(half.join("partial.txt"), "left behind\n").unwrap();
    fs::create_dir_all(half.join("target")).unwrap();
    fs::write(half.join("target/out"), "build output\n").unwrap();
    fs::remove_file(half.join(".git")).unwrap();
    fs::remove_dir_all(main.join(".git/worktrees/half")).unwrap();

    let records = main.join(".git/north/lane-rejections");
    fs::create_dir_all(&records).unwrap();
    fs::write(records.join("rejected"), "test-passes-on-base (review of ds-1)\n").unwrap();

    let project = archive.join("proj");
    fs::create_dir_all(&project).unwrap();
    let (stale, fresh) = (project.join("old.bundle.lock"), project.join("new.bundle.lock"));
    fs::write(&fresh, "").unwrap();
    fs::write(&stale, "").unwrap();
    File::options().write(true).open(&stale).unwrap().set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000 - 2 * 3600)).unwrap();

    let arguments: Vec<String> = ["--repo", "proj", "--idle-hours", "0", "--root", home.join("code").to_str().unwrap(), "--state", root.join("unlanded.txt").to_str().unwrap(), "--archive", archive.to_str().unwrap()].map(str::to_owned).to_vec();
    assert_eq!(north_lanes::lanes::lane_gc_in(&home, Some(Vec::new()), &arguments), 0);

    assert!(!rejected.exists(), "the rejected lane is reaped");
    assert!(git(&main, &["branch", "--list", "rejected"]).is_empty());
    assert!(bystander.exists() && !git(&main, &["branch", "--list", "bystander"]).is_empty(), "a lane no reviewer rejected stays");
    assert!(!half.exists(), "the half-removed lane directory is archived and removed");
    assert!(!git(&main, &["branch", "--list", "half"]).is_empty(), "its branch is left to the branch sweep");

    let index = fs::read_to_string(project.join("index.tsv")).unwrap();
    assert!(index.contains("rejected: test-passes-on-base"), "{index}");
    assert!(index.contains("git no longer registers this lane directory"), "{index}");
    let bundle = |prefix: &str| {
        let found: Vec<_> = fs::read_dir(&project).unwrap().flatten().map(|entry| entry.path()).filter(|path| path.file_name().unwrap().to_string_lossy().starts_with(prefix) && path.extension().is_some_and(|ext| ext == "bundle")).collect();
        assert_eq!(found.len(), 1, "one bundle for {prefix}");
        found[0].clone()
    };
    for (prefix, restored, file, text) in [("rejected-", "restored-rejected", "notes.txt", "uncommitted\n"), ("half-", "restored-half", "partial.txt", "left behind\n")] {
        let path = bundle(prefix);
        git(&main, &["bundle", "verify", "-q", path.to_str().unwrap()]);
        git(&main, &["fetch", "-q", path.to_str().unwrap(), &format!("refs/north-archive/{}:refs/heads/{restored}", prefix.trim_end_matches('-'))]);
        assert_eq!(git(&main, &["show", &format!("{restored}:{file}")]), text.trim_end(), "{file} rides in the {prefix} bundle");
    }

    assert!(!stale.exists(), "a bundle lock no process holds and older than an hour is cleared");
    assert!(fresh.exists(), "a recent lock stays");
}
