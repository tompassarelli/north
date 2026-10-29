use std::fs::{self, File};
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
        self.run(args, sync_status, alert_status, &[])
    }

    fn north_with(&self, args: &[&str], environment: &[(&str, &str)]) -> Output {
        self.run(args, 0, 0, environment)
    }

    fn run(&self, args: &[&str], sync_status: u8, alert_status: u8, environment: &[(&str, &str)]) -> Output {
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
        format!("ssh -o BatchMode=yes -o ConnectTimeout=20 -o ServerAliveInterval=15 -o ServerAliveCountMax=4 far nix-store --realise --quiet {glibc}"),
        format!("rsync -a --timeout=120 --mkpath {} {} far:.local/state/north/runtimes/{}/", runtime.join("north").display(), runtime.join("verify-north").display(), host.target),
        "ssh -o BatchMode=yes -o ConnectTimeout=20 -o ServerAliveInterval=15 -o ServerAliveCountMax=4 far north runtime update".to_owned(),
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
    assert!(String::from_utf8_lossy(&help.stdout).starts_with("usage: north runtime update | north runtime deploy [HOST...]"));
    let refused = host.north(&["runtime", "deploy", "far", "--bogus"], 0, 0);
    assert_eq!(refused.status.code(), Some(64));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("unknown flag --bogus") && stderr.contains("usage: north runtime"), "{stderr}");
    assert!(!host.path("alerts").exists(), "nothing was shipped");
    assert!(!host.path("state/north/runtimes").exists(), "nothing was built");
}

#[test]
fn a_promotion_roots_each_store_path_the_runtime_names_inside_the_runtime_directory() {
    let host = skewed();
    let glibc = "/nix/store/n51dhmdbik1kfrsm62j5knavmigwrl1a-glibc-2.42-84";
    script(&host.runtime(&host.target).join("north"), &format!("# {glibc}/lib/ld-linux-x86-64.so.2\n"));
    script(&host.path("bin/nix-store"), "printf '%s\\n' \"$*\" >>\"$TEST_ALERTS\"\n");
    let output = host.update(0, 0);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let root = host.runtime(&host.target).join("gcroot-n51dhmdbik1kfrsm62j5knavmigwrl1a-glibc-2.42-84");
    let expected = format!("--add-root {} --indirect --realise {glibc}", root.display());
    assert_eq!(fs::read_to_string(host.path("alerts")).unwrap().lines().collect::<Vec<_>>(), [expected]);
}

#[test]
fn a_promotion_that_cannot_root_a_store_path_leaves_the_current_runtime_selected() {
    let host = skewed();
    script(&host.runtime(&host.target).join("north"), "# /nix/store/n51dhmdbik1kfrsm62j5knavmigwrl1a-glibc-2.42-84\n");
    script(&host.path("bin/nix-store"), "exit 3\n");
    let output = host.update(0, 0);
    assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
}

#[test]
fn a_promoted_runtime_that_fails_its_health_check_is_rolled_back_before_agents_sync_runs() {
    let host = skewed();
    script(&host.runtime(&host.target).join("north"), "[[ \"$1 $2\" == 'runtime health' ]] && exit 7\nexit 0\n");
    script(&host.path("bin/agents"), "touch \"$TEST_SYNCED\"\n");
    let output = host.update(0, 0);
    assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
    assert!(!host.path("synced").exists(), "agents sync ran against an unhealthy runtime");
    let alerts = fs::read_to_string(host.path("alerts")).unwrap();
    let expected = format!("failed step health check on {}; rolled back to runtimes/{PREVIOUS}; exit status 7", host.target);
    assert!(alerts.contains(&expected), "{alerts}");
}

#[test]
fn a_commit_that_failed_its_health_check_is_held_back_for_an_hour_without_another_alert() {
    let host = skewed();
    assert_eq!(host.update(42, 0).status.code(), Some(1));
    let retried = host.update(0, 0);
    assert!(retried.status.success(), "{}", String::from_utf8_lossy(&retried.stderr));
    let stdout = String::from_utf8_lossy(&retried.stdout);
    assert!(stdout.contains(&format!("{} failed within the last hour; staying on {PREVIOUS}", &host.target[..8])), "{stdout}");
    assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
    assert_eq!(fs::read_to_string(host.path("alerts")).unwrap().lines().filter(|line| *line == "proxy").count(), 1);
    let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(61 * 60);
    File::options().write(true).open(host.path("state/north/runtime-failed")).unwrap().set_modified(hour_ago).unwrap();
    let promoted = host.update(0, 0);
    assert!(promoted.status.success(), "{}", String::from_utf8_lossy(&promoted.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
}

#[test]
fn a_deploy_that_cannot_take_the_update_lock_says_what_it_waits_on_and_fails_with_that_reason() {
    let host = Host::new();
    fs::create_dir_all(host.path("state/north")).unwrap();
    let held = File::create(host.path("state/north/runtime-update.lock")).unwrap();
    rustix::fs::flock(&held, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
    let output = host.north_with(&["runtime", "deploy", "far"], &[("NORTH_RUNTIME_STEP_SECONDS", "2")]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("waiting up to 2s for runtime-update.lock, held by another north runtime update"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("runtime-update.lock stayed held by another north runtime update for 2s"));
}

#[test]
fn a_deploy_step_that_hangs_on_a_host_is_stopped_and_named_with_its_host_and_limit() {
    let host = Host::new();
    host.built(&host.target);
    script(&host.path("bin/ssh"), "sleep 30\n");
    let output = host.north_with(&["runtime", "deploy", "far"], &[("NORTH_RUNTIME_STEP_SECONDS", "1")]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("far: nix-store --realise"), "{stdout}");
    assert!(stderr.contains("nix-store --realise on far did not finish within 1s and was stopped"), "{stderr}");
}

fn fleet(host: &Host, hosts: &str) -> PathBuf {
    let orchestration = host.path("orchestration.toml");
    fs::write(&orchestration, hosts).unwrap();
    orchestration
}

#[test]
fn a_runtime_another_host_built_is_pulled_and_its_closure_realised_instead_of_built_here() {
    let host = Host::new();
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    let glibc = "/nix/store/n51dhmdbik1kfrsm62j5knavmigwrl1a-glibc-2.42-84";
    script(
        &host.path("bin/rsync"),
        &format!("printf 'rsync %s\\n' \"$*\" >>\"$TEST_ALERTS\"\nfor program in north verify-north; do printf '#!/bin/sh\\n# {glibc}\\nexit 0\\n' >\"${{@: -1}}/$program\"; chmod +x \"${{@: -1}}/$program\"; done\n"),
    );
    script(&host.path("bin/nix-store"), "printf 'nix-store %s\\n' \"$*\" >>\"$TEST_ALERTS\"\n");
    let orchestration = fleet(&host, "[hosts.builder]\nssh = \"tom@builder\"\n");
    let output = host.north_with(
        &["runtime", "update"],
        &[("AGENTS_ORCHESTRATION", orchestration.to_str().unwrap()), ("RESIDENT_CAP_HOST", "puller")],
    );
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
    let calls = fs::read_to_string(host.path("alerts")).unwrap();
    assert!(calls.contains(&format!("tom@builder:.local/state/north/runtimes/{}/north", host.target)), "{calls}");
    assert!(calls.contains(&format!("nix-store --realise --quiet {glibc}")), "{calls}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("pulled"), "{}", String::from_utf8_lossy(&output.stdout));
}

#[test]
fn a_host_that_declares_builds_runtime_false_defers_when_no_other_host_has_built_the_commit() {
    let host = Host::new();
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    script(&host.path("bin/rsync"), "exit 23\n");
    script(&host.path("bin/ssh"), "exit 255\n");
    let orchestration = fleet(&host, "[hosts.builder]\nssh = \"tom@builder\"\n[hosts.small]\nbuilds_runtime = false\n");
    let output = host.north_with(
        &["runtime", "update"],
        &[("AGENTS_ORCHESTRATION", orchestration.to_str().unwrap()), ("RESIDENT_CAP_HOST", "small")],
    );
    assert_eq!(output.status.code(), Some(75), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stderr).contains("this host does not build runtimes"));
    assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
}

#[test]
fn a_build_whose_commit_is_no_longer_origin_main_is_abandoned_and_the_new_head_is_built() {
    let host = Host::new();
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    let skill = host.path("skill/machine-capacity");
    script(
        &host.path("bin/agents"),
        &format!("case \"$1\" in path) echo {};; sync) exit 0;; esac\n", skill.display()),
    );
    script(
        &host.path("bin/bun"),
        "n=$(cat \"$TEST_COUNT\" 2>/dev/null || echo 0)\necho $((n + 1)) >\"$TEST_COUNT\"\n[ \"$n\" = 0 ] && exec sleep 120\n\
         release=\"$HOME/.cache/cargo-target/north/release\"\nmkdir -p \"$release\"\n\
         for program in north verify-north; do printf '#!/bin/sh\\nexit 0\\n' >\"$release/$program\"; chmod +x \"$release/$program\"; done\n",
    );
    let count = host.path("count");
    let source = host.path("source");
    let home = host.path("home");
    let started = std::time::Instant::now();
    let output = std::thread::scope(|scope| {
        let run = scope.spawn(|| {
            host.north_with(
                &["runtime", "update"],
                &[("NORTH_RUNTIME_WATCH_SECONDS", "1"), ("TEST_COUNT", count.to_str().unwrap())],
            )
        });
        while !count.exists() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        git(&home, &source, &["commit", "-q", "--allow-empty", "-m", "second"]);
        run.join().unwrap()
    });
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("abandoned its build") && stdout.contains("restarting on the new origin/main"), "{stdout}");
    assert_eq!(fs::read_to_string(&count).unwrap().trim(), "2");
    let head = git(&home, &source, &["rev-parse", "HEAD"]);
    assert_eq!(host.current(), Path::new("runtimes").join(head));
    assert!(started.elapsed() < std::time::Duration::from_secs(60));
}

#[test]
fn a_follower_that_cannot_get_origin_main_promotes_the_newest_ancestor_a_peer_has_promoted() {
    let host = Host::new();
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    let source = host.path("source");
    let home = host.path("home");
    git(&home, &source, &["commit", "-q", "--allow-empty", "-m", "finished on a peer"]);
    let finished = git(&home, &source, &["rev-parse", "HEAD"]);
    git(&home, &source, &["checkout", "-q", "-b", "side"]);
    git(&home, &source, &["commit", "-q", "--allow-empty", "-m", "off main"]);
    let side = git(&home, &source, &["rev-parse", "HEAD"]);
    git(&home, &source, &["checkout", "-q", "main"]);
    git(&home, &source, &["commit", "-q", "--allow-empty", "-m", "landed, built nowhere"]);
    let main = git(&home, &source, &["rev-parse", "HEAD"]);
    script(
        &host.path("bin/ssh"),
        &format!("case \"$*\" in *tom@older*) echo runtimes/{};; *tom@newer*) echo runtimes/{finished};; *tom@side*) echo runtimes/{side};; esac\n", host.target),
    );
    script(
        &host.path("bin/rsync"),
        &format!("printf 'rsync %s\\n' \"$*\" >>\"$TEST_ALERTS\"\n[[ \"$*\" == *{main}* ]] && exit 23\nfor program in north verify-north; do printf '#!/bin/sh\\nexit 0\\n' >\"${{@: -1}}/$program\"; chmod +x \"${{@: -1}}/$program\"; done\n"),
    );
    script(&host.path("bin/nix-store"), "exit 0\n");
    let orchestration = fleet(
        &host,
        "[hosts.older]\nssh = \"tom@older\"\n[hosts.newer]\nssh = \"tom@newer\"\n[hosts.side]\nssh = \"tom@side\"\n[hosts.small]\nbuilds_runtime = false\n",
    );
    let output = host.north_with(
        &["runtime", "update"],
        &[("AGENTS_ORCHESTRATION", orchestration.to_str().unwrap()), ("RESIDENT_CAP_HOST", "small"), ("TEST_TARGET", &finished)],
    );
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(&finished));
    let calls = fs::read_to_string(host.path("alerts")).unwrap();
    assert!(calls.contains(&format!("tom@newer:.local/state/north/runtimes/{finished}/north")), "{calls}");
    assert!(host.runtime(&finished).join("tree").is_dir());
}

#[test]
fn a_build_more_than_half_done_when_origin_main_moves_is_finished_and_promoted_before_the_new_head_is_built() {
    let host = Host::new();
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    fs::write(host.path("state/north/runtime-build-seconds"), "1").unwrap();
    let skill = host.path("skill/machine-capacity");
    script(
        &host.path("bin/agents"),
        &format!("case \"$1\" in path) echo {};; sync) exit 0;; esac\n", skill.display()),
    );
    script(
        &host.path("bin/bun"),
        "n=$(cat \"$TEST_COUNT\" 2>/dev/null || echo 0)\necho $((n + 1)) >\"$TEST_COUNT\"\n[ \"$n\" = 0 ] && sleep 4\n\
         release=\"$HOME/.cache/cargo-target/north/release\"\nmkdir -p \"$release\"\n\
         for program in north verify-north; do printf '#!/bin/sh\\nexit 0\\n' >\"$release/$program\"; chmod +x \"$release/$program\"; done\n",
    );
    let count = host.path("count");
    let source = host.path("source");
    let home = host.path("home");
    let output = std::thread::scope(|scope| {
        let run = scope.spawn(|| {
            host.north_with(
                &["runtime", "update"],
                &[("NORTH_RUNTIME_WATCH_SECONDS", "1"), ("TEST_COUNT", count.to_str().unwrap())],
            )
        });
        while !count.exists() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        git(&home, &source, &["commit", "-q", "--allow-empty", "-m", "second"]);
        run.join().unwrap()
    });
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}{}", String::from_utf8_lossy(&output.stderr));
    let head = git(&home, &source, &["rev-parse", "HEAD"]);
    assert!(stdout.contains(&format!("north runtime promoted: {} (was {PREVIOUS})", &host.target[..8])), "{stdout}");
    assert!(stdout.contains(&format!("north runtime promoted: {} (was {})", &head[..8], host.target)), "{stdout}");
    assert!(!stdout.contains("abandoned"), "{stdout}");
    assert_eq!(host.current(), Path::new("runtimes").join(head));
}
