use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

pub const PREVIOUS: &str = "2222222222222222222222222222222222222222";
pub const SECRET: &str = "synthetic-fixture-bearer-token";

pub struct Host {
    pub dir: TempDir,
    pub target: String,
}

pub fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/usr/bin/env bash\n{body}")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

pub fn git(home: &Path, dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false"])
        .args(args)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

impl Host {
    pub fn new() -> Self {
        Self::landed("fixture", None)
    }

    pub fn landed(subject: &str, date: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let source = dir.path().join("source");
        fs::create_dir_all(source.join("harness/lib")).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(source.join("harness/lib/usage.py"), "").unwrap();
        git(&home, &source, &["init", "-q", "-b", "main"]);
        git(&home, &source, &["add", "."]);
        match date {
            Some(date) => {
                let dated = Command::new("git").arg("-C").arg(&source).args(["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-q", "-m", subject]).env("HOME", &home).env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_COMMITTER_DATE", date).env("GIT_AUTHOR_DATE", date).status().unwrap();
                assert!(dated.success());
            }
            None => drop(git(&home, &source, &["commit", "-q", "-m", subject])),
        }
        let target = git(&home, &source, &["rev-parse", "HEAD"]);
        git(&home, dir.path(), &["clone", "-q", "source", "checkout"]);
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        script(
            &dir.path().join("bin/agents"),
            "[[ $1 == sync && \"$(readlink \"$XDG_STATE_HOME/north/current\")\" == \"runtimes/$TEST_TARGET\" ]] || exit 91\n\
             printf 'fixture sync error: Authorization: Bearer %s\\n' \"$TEST_SECRET\"\nexit \"$TEST_SYNC_STATUS\"\n",
        );
        script(
            &dir.path().join("bin/north"),
            "[[ \"$1 $2 $3\" == 'fleet send proxy' ]] || exit 94\n\
             [[ \"$(readlink \"$XDG_STATE_HOME/north/current\")\" == \"runtimes/$TEST_PREVIOUS\" ]] || exit 92\n\
             printf '%s\\n' \"$@\" >>\"$TEST_ALERTS\"\nexit \"$TEST_EVENT_STATUS\"\n",
        );
        Self { dir, target }
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    pub fn runtime(&self, sha: &str) -> PathBuf {
        self.path("state/north/runtimes").join(sha)
    }

    pub fn built(&self, sha: &str) {
        fs::create_dir_all(self.runtime(sha)).unwrap();
        for program in ["north", "verify-north"] {
            script(&self.runtime(sha).join(program), "exit 0\n");
        }
    }

    pub fn select(&self, sha: &str) {
        symlink(Path::new("runtimes").join(sha), self.path("state/north/current")).unwrap();
    }

    pub fn lose_agents(&self) {
        fs::remove_file(self.path("bin/agents")).unwrap();
        for tool in ["git", "tar", "bash", "readlink"] {
            let found = std::env::split_paths(&std::env::var_os("PATH").unwrap()).map(|dir| dir.join(tool)).find(|path| path.is_file());
            symlink(found.unwrap(), self.path("bin").join(tool)).unwrap();
        }
    }

    pub fn current(&self) -> PathBuf {
        fs::read_link(self.path("state/north/current")).unwrap()
    }

    pub fn update(&self, sync_status: u8, alert_status: u8) -> Output {
        self.north(&["runtime", "update"], sync_status, alert_status)
    }

    pub fn north(&self, args: &[&str], sync_status: u8, alert_status: u8) -> Output {
        self.run(args, sync_status, alert_status, &[])
    }

    pub fn north_with(&self, args: &[&str], environment: &[(&str, &str)]) -> Output {
        self.run(args, 0, 0, environment)
    }

    pub fn run(&self, args: &[&str], sync_status: u8, alert_status: u8, environment: &[(&str, &str)]) -> Output {
        let isolated = !self.path("bin/agents").exists();
        let path = match isolated {
            true => self.path("bin").display().to_string(),
            false => format!("{}:{}", self.path("bin").display(), std::env::var("PATH").unwrap()),
        };
        Command::new(env!("CARGO_BIN_EXE_north"))
            .args(args)
            .env("PATH", path)
            .env("HOME", self.path("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_STATE_HOME", self.path("state"))
            .env("NORTH_HOME", self.path("checkout"))
            .env("TEST_TARGET", &self.target)
            .env("TEST_PREVIOUS", PREVIOUS)
            .env("TEST_SECRET", SECRET)
            .env("TEST_ALERTS", self.path("alerts"))
            .env("TEST_SYNCED", self.path("synced"))
            .env("TEST_SYNC_STATUS", sync_status.to_string())
            .env("TEST_EVENT_STATUS", alert_status.to_string())
            .envs(environment.iter().copied())
            .output()
            .unwrap()
    }
}

pub fn fleet(host: &Host, hosts: &str) -> PathBuf {
    let orchestration = host.path("orchestration.toml");
    fs::write(&orchestration, hosts).unwrap();
    orchestration
}
