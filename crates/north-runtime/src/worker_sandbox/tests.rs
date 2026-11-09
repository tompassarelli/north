use std::path::{Path, PathBuf};
use std::process::Command;

use landlock::RulesetStatus;

use super::{Context, confine, executable, readable, removable};

const COMMAND: &str = "NORTH_706_LOCKS_COMMAND";
const LOGINS_ONLY: &str = "NORTH_977_LOGINS_ONLY";

fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "protocol.file.allow=always"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
}

fn locks(under: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(under).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() && !path.is_symlink() {
            found.extend(locks(&path));
        } else if path.extension().is_some_and(|extension| extension == "lock") {
            found.push(path);
        }
    }
    found
}

#[test]
fn a_confined_command_child() {
    let Some(command) = std::env::var_os(COMMAND) else {
        return;
    };
    let command = command.to_string_lossy().into_owned();
    let context = Context::from_env(&command);
    let roots = std::env::var_os(LOGINS_ONLY).is_none().then(|| removable(&context));
    assert!(matches!(confine(roots.as_deref(), &readable(&context), Some(&executable(&context))), Ok(RulesetStatus::FullyEnforced)), "Landlock does not enforce here");
    let output = Command::new("bash").arg("-c").arg(&command).output().unwrap();
    assert!(output.status.success(), "{command}: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn a_sandboxed_git_fetch_leaves_no_lock_in_a_common_dir_it_cannot_remove_north_706() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("north-706-locks").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let remote = home.join("remote.git");
    let base = home.join("code/alpha/worktrees/base");
    let beta = home.join("code/beta/main");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::create_dir_all(beta.parent().unwrap()).unwrap();
    git(&home, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
    git(&base, &["init", "-q", "-b", "main"]);
    git(&base, &["commit", "-q", "--allow-empty", "-m", "one"]);
    git(&base, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&base, &["push", "-q", "origin", "main"]);
    git(&base, &["worktree", "add", "-q", "../../main", "-b", "trunk"]);
    git(&base, &["worktree", "add", "-q", "../lane", "-b", "lane"]);
    git(&home, &["clone", "-q", remote.to_str().unwrap(), beta.to_str().unwrap()]);
    git(&base, &["commit", "-q", "--allow-empty", "-m", "two"]);
    git(&base, &["push", "-q", "origin", "main"]);
    assert!(home.join("code/alpha/main/.git").is_file(), "the alpha checkout's .git must be a gitfile");

    let lane = home.join("code/alpha/worktrees/lane");
    let own = "git fetch -q origin".to_owned();
    let named = format!("cd {} && git fetch -q origin", beta.display());
    for command in [own, named] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "worker_sandbox::tests::a_confined_command_child", "--nocapture"])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", &home)
            .env(COMMAND, &command)
            .current_dir(&lane)
            .output()
            .unwrap();
        assert!(output.status.success(), "{command}: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert_eq!(locks(&home), Vec::<PathBuf>::new(), "{command} left a lock behind");
    }
    let fetched = |repository: &Path| Command::new("git").arg("-C").arg(repository).args(["rev-list", "--count", "origin/main"]).output().unwrap().stdout;
    assert_eq!(fetched(&base), b"2\n");
    assert_eq!(fetched(&beta), b"2\n");
}

#[test]
fn a_command_run_from_main_may_build_in_the_done_when_lane_north_task_check_uses() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("north-done-when-lane").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let main = home.join("code/alpha/main");
    std::fs::create_dir_all(&main).unwrap();
    std::fs::create_dir_all(home.join("code/alpha/worktrees")).unwrap();
    let lane = "../worktrees/done-when/target";
    let command = format!("mkdir -p {lane}/a && mv {lane}/a {lane}/b && rm -r {lane}");
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "worker_sandbox::tests::a_confined_command_child", "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", &home)
        .env(COMMAND, &command)
        .current_dir(&main)
        .output()
        .unwrap();
    assert!(output.status.success(), "{command}: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}

#[test]
fn no_agent_command_of_any_role_can_read_or_link_away_the_operators_gh_login_north_969_north_977() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("north-969-gh-login").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let gh = home.join(".config/gh");
    std::fs::create_dir_all(&gh).unwrap();
    std::fs::write(gh.join("hosts.yml"), "github.com:\n    oauth_token: personal\n").unwrap();
    std::fs::write(gh.join("config.yml"), "git_protocol: https\n").unwrap();
    std::os::unix::fs::symlink(gh.join("hosts.yml"), home.join("hosts-link.yml")).unwrap();
    let command = "! cat ~/.config/gh/hosts.yml && ! cat ~/hosts-link.yml && ! ln ~/.config/gh/hosts.yml /tmp/north-969-leak-$$ && cat ~/.config/gh/config.yml";
    for logins_only in [false, true] {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.args(["--exact", "worker_sandbox::tests::a_confined_command_child", "--nocapture"]).env_clear().env("PATH", std::env::var_os("PATH").unwrap()).env("HOME", &home).env(COMMAND, command).current_dir(&home);
        if logins_only {
            child.env(LOGINS_ONLY, "1");
        }
        let output = child.output().unwrap();
        assert!(output.status.success(), "logins only {logins_only}, {command}: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    }
}

fn runnable(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    crate::foundation::shim::write_script(path, "exit 0\n");
}

// KEEP(bug): a lane's test binary or package script run by its path ran outside depbox, so the code a lane's dependencies bring ran past depbox whenever an agent typed its path (south#93)
#[test]
fn a_lane_test_binary_run_by_path_outside_depbox_is_refused() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("south-93-build-output").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let lane = home.join("code/alpha/worktrees/lane");
    let fresh = home.join("code/alpha/worktrees/fresh");
    let built = ["target/debug/deps/it-0123", "node_modules/.bin/vitest", "web/node_modules/.bin/tsc"];
    for path in built.iter().chain(&["bin/tool"]) {
        runnable(&lane.join(path));
        assert!(Command::new(lane.join(path)).status().unwrap().success(), "{path} does not run outside the domain");
    }
    std::fs::create_dir_all(&fresh).unwrap();
    let refused: Vec<String> = built.iter().map(|path| format!("{{ ./{path}; test $? = 126; }}")).collect();
    let command = format!("{} && ./bin/tool && cd {} && mkdir -p target/debug && cp {}/bin/tool target/debug/fresh && {{ ./target/debug/fresh; test $? = 126; }}", refused.join(" && "), fresh.display(), lane.display());
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "worker_sandbox::tests::a_confined_command_child", "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", &home)
        .env(COMMAND, &command)
        .current_dir(&lane)
        .output()
        .unwrap();
    assert!(output.status.success(), "{command}: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}

#[test]
fn the_outside_broker_runs_only_north_s_fixed_verbs_with_an_environment_that_carries_no_injected_code() {
    let words = |text: &str| -> Vec<String> { text.split(' ').map(str::to_owned).collect() };
    for refused in ["sh -c id", "depbox run", "depbox exec -- cargo", "lane exec x", "--worker-sandbox -- sh", "capacity host run --class moderate", "capacity host run --class moderate --", "capacity host status"] {
        assert!(!super::broker::permitted(&words(refused)), "{refused}");
    }
    for allowed in ["depbox run -- cargo check", "depbox fetch -- cargo fetch", "lane unlock x", "lane stray y", "lane remove z", "--worker-sandbox --built -- ./t", "capacity host run --class moderate --owner o --timeout-seconds 9 -- cargo check"] {
        assert!(super::broker::permitted(&words(allowed)), "{allowed}");
    }
    let sent = [("LD_PRELOAD", "/tmp/x.so"), ("GIT_SSH_COMMAND", "sh"), ("GIT_CONFIG_COUNT", "9"), ("NORTH_WORKER_SANDBOXED", "1"), ("GIT_SSL_CAINFO", "/ca"), ("GIT_AUTHOR_NAME", "a"), ("NORTH_SOCKETS_HIDDEN", "1"), ("CARGO_HOME", "/c"), ("PATH", "bin:/usr/bin::/opt/x")];
    let environment = super::broker::environment(sent.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect());
    let get = |name: &str| environment.iter().find(|(own, _)| own == name).map(|(_, value)| value.as_str());
    assert_eq!((get("LD_PRELOAD"), get("GIT_SSH_COMMAND"), get("NORTH_WORKER_SANDBOXED"), get("GIT_SSL_CAINFO"), get("CARGO_HOME"), get("PATH")), (None, None, None, Some("/ca"), Some("/c"), Some("/usr/bin:/opt/x")));
    assert_ne!(get("GIT_CONFIG_COUNT"), Some("9"));
    assert_eq!((get("GIT_AUTHOR_NAME"), get("NORTH_SOCKETS_HIDDEN")), (Some("a"), None));
    let north = Path::new("/n/north");
    let wrapped = super::broker::confined(north, words("capacity host run --class heavy --owner o -- cargo test -p x"));
    assert_eq!(wrapped.join(" "), "capacity host run --class heavy --owner o -- /n/north --worker-sandbox --program -- cargo test -p x");
}
