use std::fs;
use north_core::foundation::shim;
use std::path::Path;
use std::process::{Command, Output};

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn commit(repo: &Path, message: &str) -> String {
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", message]);
    git(repo, &["rev-parse", "HEAD"])
}

struct Rig {
    root: tempfile::TempDir,
}

impl Rig {
    fn new(unit: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("origin.git");
        git(root.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        let checkout = root.path().join("nixos-config/main");
        git(root.path(), &["clone", "-q", origin.to_str().unwrap(), checkout.to_str().unwrap()]);
        write(&checkout.join("modules/unit/default.nix"), unit);
        commit(&checkout, "the deployed unit");
        git(&checkout, &["push", "-q", "origin", "HEAD:main"]);
        let stubs = root.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        shim::write_script(&stubs.join("systemctl"), "exit 0\n");
        Self { root }
    }

    fn scan(&self, repo: &Path, base: &str, head: &str) -> Output {
        let path = format!("{}:{}", self.root.path().join("stubs").display(), std::env::var("PATH").unwrap());
        Command::new(env!("CARGO_BIN_EXE_north"))
            .env("PATH", path)
            .args(["bin-references", "--repo"])
            .arg(repo)
            .args(["--base", base, "--head", head, "--nixos-config"])
            .arg(self.root.path().join("nixos-config"))
            .arg("--south")
            .arg(self.root.path().join("no-south"))
            .output()
            .unwrap()
    }
}

fn north_repo(root: &Path, files: &[(&str, &str)]) -> (std::path::PathBuf, String) {
    let repo = root.join("north");
    git(root, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
    files.iter().for_each(|(path, text)| write(&repo.join(path), text));
    let base = commit(&repo, "base");
    (repo, base)
}

#[test]
fn a_lane_removing_bin_lane_gc_is_refused_while_the_deployed_unit_runs_it() {
    let rig = Rig::new("{\n  ExecStart = \"${northBin}/lane-gc --root /r\";\n}\n");
    let (repo, base) = north_repo(rig.root.path(), &[("bin/lane-gc", "#!/bin/sh\n"), ("src/lib.rs", "")]);
    fs::remove_file(repo.join("bin/lane-gc")).unwrap();
    let head = commit(&repo, "remove lane-gc");
    let refused = rig.scan(&repo, &base, &head);
    let report = String::from_utf8_lossy(&refused.stdout).into_owned();
    assert_eq!(refused.status.code(), Some(1), "{report}");
    assert!(report.contains("bin/lane-gc is deleted here, but nixos-config origin/main still runs tree/bin/lane-gc at modules/unit/default.nix:2"), "{report}");
}

#[test]
fn a_lane_removing_a_flag_the_deployed_unit_passes_is_refused() {
    let rig = Rig::new("{\n  ExecStart = \"${northBin}/north land serve --all --repo ${homeDir}/code/north/main\";\n}\n");
    let (repo, base) = north_repo(rig.root.path(), &[("src/land.rs", "fn f() { let _ = [\"--all\", \"--repo\"]; }\n")]);
    write(&repo.join("src/land.rs"), "fn f() { let _ = [\"--repo\"]; }\n");
    let head = commit(&repo, "drop --all");
    let refused = rig.scan(&repo, &base, &head);
    let report = String::from_utf8_lossy(&refused.stdout).into_owned();
    assert_eq!(refused.status.code(), Some(1), "{report}");
    assert!(report.contains("the north flag --all is removed here, but nixos-config origin/main still passes it at modules/unit/default.nix:2"), "{report}");
}

#[test]
fn a_lane_that_keeps_what_the_deployed_units_run_passes() {
    let rig = Rig::new("{\n  ExecStart = \"${northBin}/north land serve --repo ${homeDir}/code/north/main\";\n}\n");
    let (repo, base) = north_repo(rig.root.path(), &[("src/land.rs", "fn f() { let _ = [\"--all\", \"--repo\"]; }\n")]);
    write(&repo.join("src/land.rs"), "fn f() { let _ = [\"--repo\"]; }\n");
    let head = commit(&repo, "drop --all");
    let passed = rig.scan(&repo, &base, &head);
    assert_eq!(passed.status.code(), Some(0), "{}", String::from_utf8_lossy(&passed.stdout));
}
