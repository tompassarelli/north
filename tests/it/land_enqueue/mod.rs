use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod clippy;
mod secrets;

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    root: PathBuf,
    _directory: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::seeded(&[("file", "base\n")])
    }

    fn seeded(files: &[(&str, &str)]) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let (origin, repo, state) = (root.join("origin.git"), root.join("repo"), root.join("state"));
        git(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        git(&root, &["init", "-q", "-b", "main", "repo"]);
        git(&repo, &["config", "user.name", "Enqueue scenario"]);
        git(&repo, &["config", "user.email", "enqueue@example.invalid"]);
        git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        for (name, text) in files {
            let path = repo.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "Start the repository"]);
        git(&repo, &["push", "-q", "origin", "main"]);
        git(&repo, &["switch", "-qc", "lane"]);
        Fixture { root, _directory: directory, repo, state }
    }

    fn commit(&self, repo: &Path, name: &str, message: &str) {
        fs::write(repo.join(name), format!("{name}\n")).unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-qm", message]);
    }
}

fn north_enqueue_with(fixture: &Fixture, environment: &[(&str, &Path)]) -> (std::process::Output, std::time::Duration) {
    north_enqueue_run(fixture, &[], environment)
}

fn north_enqueue_run(fixture: &Fixture, arguments: &[&str], environment: &[(&str, &Path)]) -> (std::process::Output, std::time::Duration) {
    let started = std::time::Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["land", "enqueue", "--repo"])
        .arg(&fixture.repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(arguments)
        .env("XDG_STATE_HOME", &fixture.state)
        .env("NORTH_LAND_NORTH", env!("CARGO_BIN_EXE_north"))
        .env("SAFE_PUSH_SOUTH", fixture.root.join("no-south"))
        .env_remove("SAFE_PUSH_DENYLIST")
        .envs(environment.iter().copied())
        .env_remove("NORTH_LAND_SERVING")
        .env_remove("SAFE_PUSH_LAND_CAPACITY")
        .env_remove("SAFE_PUSH_LAND_OFFLOAD_HOST")
        .env_remove("SAFE_PUSH_LAND_RUNNER")
        .output()
        .unwrap();
    (output, started.elapsed())
}

impl Fixture {
}

fn stub_gh(fixture: &Fixture, answer: &str) -> PathBuf {
    let bin = fixture.root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    north_core::foundation::shim::write_executable(&bin.join("gh"), format!("#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >>{}\n{answer}\n", fixture.root.join("gh-calls").display()));
    PathBuf::from(format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()))
}

fn written(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else { return Vec::new() };
    entries.map(|entry| entry.unwrap().path()).flat_map(|path| if path.is_dir() { written(&path) } else { vec![path] }).collect()
}
