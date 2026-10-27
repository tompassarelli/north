use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const PREVIOUS: &str = "2222222222222222222222222222222222222222";
const SECRET: &str = "synthetic-fixture-bearer-token";

struct Host {
    dir: TempDir,
    target: String,
}

fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/usr/bin/env bash\n{body}")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn git(home: &Path, dir: &Path, args: &[&str]) -> String {
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
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let source = dir.path().join("source");
        fs::create_dir_all(source.join("harness/lib")).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(source.join("harness/lib/usage.py"), "").unwrap();
        git(&home, &source, &["init", "-q", "-b", "main"]);
        git(&home, &source, &["add", "."]);
        git(&home, &source, &["commit", "-q", "-m", "fixture"]);
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

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn runtime(&self, sha: &str) -> PathBuf {
        self.path("state/north/runtimes").join(sha)
    }

    fn built(&self, sha: &str) {
        fs::create_dir_all(self.runtime(sha)).unwrap();
        for program in ["north", "verify-north"] {
            script(&self.runtime(sha).join(program), "exit 0\n");
        }
    }

    fn select(&self, sha: &str) {
        symlink(Path::new("runtimes").join(sha), self.path("state/north/current")).unwrap();
    }

    fn lose_agents(&self) {
        fs::remove_file(self.path("bin/agents")).unwrap();
        for tool in ["git", "tar", "bash", "readlink"] {
            let found = std::env::split_paths(&std::env::var_os("PATH").unwrap()).map(|dir| dir.join(tool)).find(|path| path.is_file());
            symlink(found.unwrap(), self.path("bin").join(tool)).unwrap();
        }
    }

    fn current(&self) -> PathBuf {
        fs::read_link(self.path("state/north/current")).unwrap()
    }

    fn update(&self, sync_status: u8, alert_status: u8) -> Output {
        self.north(&["runtime", "update"], sync_status, alert_status)
    }

    fn north(&self, args: &[&str], sync_status: u8, alert_status: u8) -> Output {
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
            .env("TEST_SYNC_STATUS", sync_status.to_string())
            .env("TEST_EVENT_STATUS", alert_status.to_string())
            .output()
            .unwrap()
    }
}

fn skewed() -> Host {
    let host = Host::new();
    host.built(&host.target);
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    host
}

#[test]
fn a_promoted_runtime_carries_the_tree_of_its_own_commit() {
    let host = skewed();
    let output = host.update(0, 0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
    assert!(host.runtime(&host.target).join("tree/harness/lib/usage.py").is_file());
    assert!(!host.runtime(&host.target).join("tree.partial").exists());
    assert!(stdout.contains(&format!("north runtime promoted: {} (was {PREVIOUS})", &host.target[..8])), "{stdout}");
    assert!(!host.path("alerts").exists());
}

#[test]
fn a_failed_sync_restores_the_previous_runtime_and_alerts_once_without_the_log() {
    for (alert_status, launched) in [(0, true), (43, true), (0, false)] {
        let host = skewed();
        if !launched {
            host.lose_agents();
        }
        let output = host.update(42, alert_status);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
        let alerts = fs::read_to_string(host.path("alerts")).unwrap();
        let alert = alerts.lines().collect::<Vec<_>>();
        assert_eq!(alert[..3], ["fleet", "send", "proxy"]);
        assert_eq!(alert.len(), 4, "{alerts}");
        let expected = format!("failed step agents sync on {}; rolled back to runtimes/{PREVIOUS}; exit status {}; local log: ", host.target, if launched { 42 } else { 127 });
        assert!(alert[3].contains(&expected), "{}", alert[3]);
        assert!(!alert[3].contains(SECRET) && !alert[3].contains("fixture sync error"));
        let log = alert[3].rsplit("local log: ").next().unwrap();
        assert_eq!(fs::metadata(log).unwrap().permissions().mode() & 0o777, 0o600);
        let logged = if launched { SECRET } else { "agents sync did not run" };
        assert!(fs::read_to_string(log).unwrap().contains(logged));
        assert_eq!(stderr.contains("rollback alert delivery failed"), alert_status != 0, "{stderr}");
    }
}

#[test]
fn a_deploy_ships_the_built_runtime_and_its_store_references_then_promotes_remotely() {
    let host = Host::new();
    host.built(&host.target);
    let glibc = "/nix/store/n51dhmdbik1kfrsm62j5knavmigwrl1a-glibc-2.42-84";
    script(&host.runtime(&host.target).join("north"), &format!("# {glibc}/lib/ld-linux-x86-64.so.2\n"));
    for tool in ["ssh", "rsync"] {
        script(&host.path("bin").join(tool), &format!("printf '{tool} %s\\n' \"$*\" >>\"$TEST_ALERTS\"\n"));
    }
    let output = host.north(&["runtime", "deploy", "far"], 0, 0);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let runtime = host.runtime(&host.target);
    let expected = [
        format!("ssh -o BatchMode=yes far nix-store --realise --quiet {glibc}"),
        format!("rsync -a --mkpath {} {} far:.local/state/north/runtimes/{}/", runtime.join("north").display(), runtime.join("verify-north").display(), host.target),
        "ssh -o BatchMode=yes far north runtime update".to_owned(),
    ];
    assert_eq!(fs::read_to_string(host.path("alerts")).unwrap().lines().collect::<Vec<_>>(), expected);
    assert!(!host.path("state/north/current").exists());
}

#[test]
fn a_deploy_with_help_prints_usage_and_with_an_unknown_flag_is_refused_before_any_build_or_ssh() {
    let host = Host::new();
    for tool in ["ssh", "rsync"] {
        script(&host.path("bin").join(tool), &format!("printf '{tool} %s\\n' \"$*\" >>\"$TEST_ALERTS\"\n"));
    }
    let help = host.north(&["runtime", "deploy", "--help"], 0, 0);
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).starts_with("usage: north runtime update | north runtime deploy HOST..."));
    let refused = host.north(&["runtime", "deploy", "far", "--bogus"], 0, 0);
    assert_eq!(refused.status.code(), Some(64));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("unknown flag --bogus") && stderr.contains("usage: north runtime"), "{stderr}");
    assert!(!host.path("alerts").exists(), "nothing was shipped");
    assert!(!host.path("state/north/runtimes").exists(), "nothing was built");
}
