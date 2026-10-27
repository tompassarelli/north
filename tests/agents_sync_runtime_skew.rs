use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

const SCRIPT: &str = "harness/skills/machine-capacity/scripts/machine-capacity.mjs";
const SKILL: &str = "---\nname: machine-capacity\ndescription: Bound local work.\ngrounded: 2026-10-01\nwritten: 2026-10-01\n---\n";
const OPERATOR: &str = r#"{
  "rootOrder": ["machine-capacity"],
  "baselines": [],
  "providerSupport": [],
  "registrations": {"machine-capacity": {"kind": "skill", "owner": {"repo": "north", "path": "harness/skills/machine-capacity/SKILL.md"}}},
  "activation": {"machine-capacity": {"distributions": [{"type": "skill", "targets": ["shared"]}]}}
}"#;

struct Host {
    dir: TempDir,
    running: String,
    newer: String,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false"])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn commit(repo: &Path, script: &str) -> String {
    write(&repo.join("harness/lib/usage.py"), "");
    write(&repo.join("agent-machinery/catalog.json"), "{\"units\": []}\n");
    write(&repo.join("harness/skills/machine-capacity/SKILL.md"), SKILL);
    write(&repo.join(SCRIPT), script);
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", script]);
    git(repo, &["rev-parse", "HEAD"])
}

impl Host {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "-q", "-b", "main"]);
        let running = commit(&source, "export const protocol = 1;\n");
        let checkout = dir.path().join("home/code/north/main");
        git(dir.path(), &["clone", "-q", "source", "home/code/north/main"]);
        let newer = commit(&source, "export const protocol = 2;\n");
        git(&checkout, &["fetch", "-q"]);
        git(&checkout, &["merge", "-q", "--ff-only", "origin/main"]);
        let host = Self { dir, running, newer };
        host.build(&host.running.clone());
        host.export_tree(&host.running.clone());
        host.select(&host.running.clone());
        write(&host.path("operator/catalog-config.json"), OPERATOR);
        write(&host.path("state/north/agents/gen-seed/activation.json"), "{\"units\": [], \"permissions\": {\"machine-capacity\": \"on\"}}\n");
        symlink("gen-seed", host.path("state/north/agents/current")).unwrap();
        host
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn runtime(&self, sha: &str) -> PathBuf {
        self.path("state/north/runtimes").join(sha)
    }

    fn build(&self, sha: &str) {
        fs::create_dir_all(self.runtime(sha)).unwrap();
        for program in ["north", "verify-north"] {
            fs::copy(env!("CARGO_BIN_EXE_north"), self.runtime(sha).join(program)).unwrap();
        }
    }

    fn export_tree(&self, sha: &str) {
        let tree = self.runtime(sha).join("tree");
        fs::create_dir_all(&tree).unwrap();
        let mut archive = Command::new("git")
            .arg("-C")
            .arg(self.path("home/code/north/main"))
            .args(["archive", "--format=tar", sha])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let extracted = Command::new("tar").arg("-x").arg("-C").arg(&tree).stdin(archive.stdout.take().unwrap()).status().unwrap();
        assert!(archive.wait().unwrap().success() && extracted.success());
    }

    fn select(&self, sha: &str) {
        let link = self.path("state/north/current");
        let _ = fs::remove_file(&link);
        symlink(Path::new("runtimes").join(sha), link).unwrap();
    }

    fn north(&self, args: &[&str]) -> Output {
        Command::new(self.path("state/north/current/north"))
            .args(args)
            .env_remove("NORTH_HOME")
            .env_remove("NORTH_REPO_ROOTS")
            .env("PATH", format!("{}:{}", self.path("bin").display(), std::env::var("PATH").unwrap()))
            .env("HOME", self.path("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_STATE_HOME", self.path("state"))
            .env("NORTH_AGENT_STATE_ROOT", self.path("state/north/agents"))
            .env("AGENTS_OPERATOR_DIR", self.path("operator"))
            .output()
            .unwrap()
    }

    fn sync(&self) -> Output {
        let output = self.north(&["config", "agents", "sync"]);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        output
    }

    fn projected(&self) -> String {
        fs::read_to_string(self.path("state/north/agents/current/skills/shared/machine-capacity/scripts/machine-capacity.mjs")).unwrap()
    }

    fn projected_revision(&self) -> String {
        let activation = fs::read_to_string(self.path("state/north/agents/current/activation.json")).unwrap();
        let activation: serde_json::Value = serde_json::from_str(&activation).unwrap();
        activation["units"][0]["ownerProvenance"]["revision"].as_str().unwrap().to_owned()
    }

    fn script_at(&self, sha: &str) -> String {
        git(&self.path("home/code/north/main"), &["show", &format!("{sha}:{SCRIPT}")]) + "\n"
    }

    fn agents_stub(&self) {
        let path = self.path("bin/agents");
        write(&path, "#!/usr/bin/env bash\n[[ $1 == sync ]] || exit 91\nexec \"$XDG_STATE_HOME/north/current/north\" config agents sync\n");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn a_sync_while_main_is_ahead_of_the_active_runtime_projects_the_runtimes_commit() {
    let host = Host::new();
    assert_ne!(host.script_at(&host.running), host.script_at(&host.newer));
    assert_eq!(host.script_at(&host.newer), fs::read_to_string(host.path("home/code/north/main").join(SCRIPT)).unwrap());

    host.sync();

    assert_eq!(host.projected(), host.script_at(&host.running));
    assert_eq!(host.projected_revision(), host.running);
}

#[test]
fn promotion_moves_the_projected_skill_scripts_to_the_new_commit() {
    let host = Host::new();
    host.sync();
    assert_eq!(host.projected(), host.script_at(&host.running));
    host.build(&host.newer.clone());
    host.agents_stub();

    let output = host.north(&["runtime", "update"]);

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read_link(host.path("state/north/current")).unwrap(), Path::new("runtimes").join(&host.newer));
    assert_eq!(host.projected(), host.script_at(&host.newer));
    assert_eq!(host.projected_revision(), host.newer);
}
