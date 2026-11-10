use std::path::{Path, PathBuf};
use std::process::Command;

use landlock::RulesetStatus;

use super::{Context, confine, executable, readable, removable};

const COMMAND: &str = "NORTH_706_LOCKS_COMMAND";
const LOGINS_ONLY: &str = "NORTH_977_LOGINS_ONLY";
const HAND_OFF_TARGET: &str = "NORTH_975_HAND_OFF_TARGET";
const OUTSIDE_MARKER: &str = "NORTH_CAPACITY_LOOP_MARKER";

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
    assert!(matches!(confine(roots.as_deref(), &readable(&context), Some(&executable(&context)), None), Ok(RulesetStatus::FullyEnforced)), "Landlock does not enforce here");
    let output = Command::new("bash").arg("-c").arg(&command).output().unwrap();
    assert!(output.status.success(), "{command}: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn a_hand_off_child() {
    let Some(target) = std::env::var_os(HAND_OFF_TARGET) else {
        return;
    };
    let _ = super::hand_off(&["rm".to_owned(), "-f".to_owned(), target.to_string_lossy().into_owned()]);
    std::process::exit(126);
}

#[test]
fn hand_off_removal_is_refused() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("north-975-hand-off").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let main = home.join("code/alpha/main");
    std::fs::create_dir_all(home.join("code/alpha/worktrees")).unwrap();
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    let kept = main.join("kept.txt");
    let allowed_dir = tempfile::Builder::new().prefix("north-975-allowed").tempdir().unwrap();
    let allowed = allowed_dir.path().join("file");
    std::fs::write(&kept, "kept").unwrap();
    std::fs::write(&allowed, "allowed").unwrap();
    let hand_off = |target: &Path| {
        let roots = super::broker::hand_off_roots(&["depbox".to_owned(), "run".to_owned(), "--".to_owned(), "rm".to_owned(), "-f".to_owned(), target.to_string_lossy().into_owned()], &main, &[("HOME".to_owned(), home.to_string_lossy().into_owned())]);
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "worker_sandbox::tests::a_hand_off_child", "--nocapture"])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env(super::REMOVAL_ROOTS, serde_json::to_string(&roots).unwrap())
            .env(HAND_OFF_TARGET, target)
            .output()
            .unwrap()
    };
    let refused = hand_off(&kept);
    assert!(!refused.status.success() && kept.exists(), "a hand-off removed a path outside the worker's trees: {}{}", String::from_utf8_lossy(&refused.stdout), String::from_utf8_lossy(&refused.stderr));
    let removed = hand_off(&allowed);
    assert!(removed.status.success() && !allowed.exists(), "a hand-off could not remove beneath the worker's git tree: {}{}", String::from_utf8_lossy(&removed.stdout), String::from_utf8_lossy(&removed.stderr));
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
    for allowed in ["depbox run -- cargo check", "depbox fetch -- cargo fetch", "lane unlock x", "lane stray y", "lane remove z", "home move plan.tsv", "--worker-sandbox --built -- ./t", "--worker-sandbox --lane -- /bin/sh -c true", "capacity host run --class moderate --owner o --timeout-seconds 9 -- cargo check"] {
        assert!(super::broker::permitted(&words(allowed)), "{allowed}");
    }
    let sent = [("LD_PRELOAD", "/tmp/x.so"), ("GIT_SSH_COMMAND", "sh"), ("GIT_CONFIG_COUNT", "9"), ("NORTH_WORKER_SANDBOXED", "1"), ("GIT_SSL_CAINFO", "/ca"), ("GIT_AUTHOR_NAME", "a"), ("NORTH_SOCKETS_HIDDEN", "1"), ("NORTH_WORKER_REMOVAL_ROOTS", "[\"/\"]"), ("CARGO_HOME", "/c"), ("PATH", "bin:/usr/bin::/opt/x"), ("XDG_RUNTIME_DIR", "/forged"), ("XDG_STATE_HOME", "/tmp/north-opencode-1/state"), ("XDG_CACHE_HOME", "/tmp/north-opencode-1/cache"), ("XDG_DATA_HOME", "/tmp/north-opencode-1/data"), ("XDG_CONFIG_HOME", "/tmp/north-opencode-1/config"), ("HOME", "/home/a/elsewhere")];
    let homes = [("XDG_STATE_HOME", "/home/a/.local/state"), ("XDG_CACHE_HOME", "/home/a/.cache"), ("XDG_DATA_HOME", "/home/a/.local/share"), ("XDG_CONFIG_HOME", "/home/a/.config"), ("HOME", "/home/a")].map(|(name, value)| (name.to_owned(), value.to_owned()));
    let environment = super::broker::environment(sent.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect(), Some(OPEN_SCOPE), Path::new("/run/user/1000"), &homes);
    let get = |name: &str| environment.iter().find(|(own, _)| own == name).map(|(_, value)| value.as_str());
    assert_eq!((get("LD_PRELOAD"), get("GIT_SSH_COMMAND"), get("NORTH_WORKER_SANDBOXED"), get("GIT_SSL_CAINFO"), get("CARGO_HOME"), get("PATH")), (None, None, None, Some("/ca"), Some("/c"), Some("/usr/bin:/opt/x")));
    assert_ne!(get("GIT_CONFIG_COUNT"), Some("9"));
    assert_eq!((get("GIT_AUTHOR_NAME"), get("NORTH_SOCKETS_HIDDEN"), get("NORTH_WORKER_REMOVAL_ROOTS")), (Some("a"), None, None));
    assert_eq!(environment.iter().filter(|(name, _)| name == "XDG_RUNTIME_DIR").map(|(_, value)| value.as_str()).collect::<Vec<_>>(), ["/run/user/1000"], "an outside command took the client's runtime directory, so an OpenCode worker that clears it got no user manager and no capacity lease (north#1023)");
    assert_eq!(environment.iter().filter(|(name, _)| name.starts_with("XDG_") && name != "XDG_RUNTIME_DIR").map(|(name, value)| (name.as_str(), value.as_str())).collect::<Vec<_>>(), [("XDG_STATE_HOME", "/home/a/.local/state"), ("XDG_CACHE_HOME", "/home/a/.cache"), ("XDG_DATA_HOME", "/home/a/.local/share"), ("XDG_CONFIG_HOME", "/home/a/.config")], "a brokered build took the client's scratch homes, so its capacity admissions landed outside the host's decision ledger (north#1030)");
    assert_eq!(environment.iter().filter(|(name, _)| name == "HOME").map(|(_, value)| value.as_str()).collect::<Vec<_>>(), ["/home/a"], "a brokered command took the client's HOME instead of the broker's (north#992)");
    let north = Path::new("/n/north");
    let wrapped = super::broker::confined(north, words("capacity host run --class heavy --owner o -- cargo test -p x"));
    assert_eq!(wrapped.join(" "), "capacity host run --class heavy --owner o -- /n/north --worker-sandbox --program -- cargo test -p x");
}

#[test]
fn the_broker_names_the_host_s_homes_and_falls_back_to_home_s_xdg_defaults() {
    let defaults = super::broker::homes(Some(PathBuf::from("/home/a")), |_| None);
    assert_eq!(defaults, [("XDG_STATE_HOME", "/home/a/.local/state"), ("XDG_CACHE_HOME", "/home/a/.cache"), ("XDG_DATA_HOME", "/home/a/.local/share"), ("XDG_CONFIG_HOME", "/home/a/.config")].map(|(name, value)| (name.to_owned(), value.to_owned())).to_vec());
    let named = super::broker::homes(None, |name| (name == "XDG_STATE_HOME").then(|| PathBuf::from("/srv/state")));
    assert_eq!(named.iter().find(|(name, _)| name == "XDG_STATE_HOME").map(|(_, value)| value.as_str()), Some("/srv/state"), "a home the host set was replaced by a default");
}

const OPEN_SCOPE: &str = "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-session-77c49ab1-cccb-4a4d-8623-d56cb8891ace.scope\n";
const LANE_SCOPE: &str = "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-session-lane-77c49ab1-cccb-4a4d-8623-d56cb8891ace.scope\n";

#[test]
fn a_lane_only_caller_without_its_marker_still_gets_lane_reads() {
    let sent = || vec![("PATH".to_owned(), "/usr/bin".to_owned()), ("HOME".to_owned(), "/home/a".to_owned())];
    let homes = [("XDG_STATE_HOME".to_owned(), "/home/a/.local/state".to_owned()), ("XDG_CACHE_HOME".to_owned(), "/home/a/.cache".to_owned()), ("XDG_DATA_HOME".to_owned(), "/home/a/.local/share".to_owned()), ("XDG_CONFIG_HOME".to_owned(), "/home/a/.config".to_owned())];
    let marker = |environment: &[(String, String)]| environment.iter().find(|(name, _)| name == super::LANE_ONLY).map(|(_, value)| value.clone());
    assert_eq!(marker(&super::broker::environment(sent(), Some(LANE_SCOPE), Path::new("/run/user/1000"), &homes)), Some("1".to_owned()), "a lane-only scope's caller without its marker got wide reads");
    assert_eq!(marker(&super::broker::environment(sent(), None, Path::new("/run/user/1000"), &homes)), Some("1".to_owned()), "a caller whose scope cannot be read got wide reads");
    assert_eq!(marker(&super::broker::environment(sent(), Some(OPEN_SCOPE), Path::new("/run/user/1000"), &homes)), None);
    let forged = sent().into_iter().chain([(super::LANE_ONLY.to_owned(), "1".to_owned())]).collect();
    assert_eq!(marker(&super::broker::environment(forged, Some(OPEN_SCOPE), Path::new("/run/user/1000"), &homes)), Some("1".to_owned()), "a caller may narrow itself");
}

#[test]
fn a_worker_may_not_remove_the_transcript_stores_or_the_ledger_beside_its_scratch_north_1041() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let root = tempfile::Builder::new().prefix("north-1041-transcripts").tempdir_in(outside_tmp).unwrap();
    let home = root.path().canonicalize().unwrap();
    let state = home.join(".local/state");
    let main = home.join("code/north/main");
    for directory in [state.join("threads"), state.join("convo"), state.join("north/profiles"), state.join("north/fleet"), state.join("north/land"), main.clone()] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let context = Context { home, cwd: main, uid: 0, tmpdir: None, cache: None, config: None, runtime: None, cargo_home: None, cargo_target: None, named_lanes: Vec::new(), named_projects: Vec::new(), runtimes: None };
    let roots = removable(&context);
    for store in ["threads", "convo", "north/profiles", "north/fleet"] {
        let store = state.join(store).canonicalize().unwrap();
        assert!(!roots.iter().any(|root| store.starts_with(root) || root.starts_with(&store)), "a worker may remove the transcript store {}", store.display());
    }
    let scratch = state.join("north/land").canonicalize().unwrap();
    assert!(roots.iter().any(|root| root == &scratch), "a worker may no longer remove its own lock files under {}", scratch.display());
}

#[test]
fn a_lane_only_domain_reads_the_north_runtime_and_not_the_state_beside_it() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    let state = home.join(".local/state/north");
    let runtimes = state.join("runtimes");
    for directory in [runtimes.join("a/tree/bin"), state.join("profiles"), home.join("code/north/main")] {
        std::fs::create_dir_all(directory).unwrap();
    }
    std::os::unix::fs::symlink("runtimes/a", state.join("current")).unwrap();
    let context = Context { home: home.clone(), cwd: home.join("code/north/main"), uid: 0, tmpdir: None, cache: None, config: None, runtime: None, cargo_home: None, cargo_target: None, named_lanes: Vec::new(), named_projects: Vec::new(), runtimes: Some(runtimes.clone()) };
    let hidden: Vec<PathBuf> = super::lane_only::hidden(&context, &[]).iter().map(|path| path.canonicalize().unwrap_or_else(|_| path.clone())).collect();
    assert!(!hidden.iter().any(|path| runtimes.join("a/tree/bin").starts_with(path)), "the north runtime is hidden: {hidden:?}");
    assert!(hidden.contains(&state.join("profiles")), "the agent state beside the runtime is readable: {hidden:?}");
}

#[test]
fn namespaces_laid_before_a_nixos_activation_moved_ssh_config_are_not_reused_north_1083() {
    use super::broker::Laid;
    let socket = PathBuf::from("/run/user/1000/bus");
    let before = Laid { hidden: vec![socket.clone()], ssh_config: Some(PathBuf::from("/nix/store/d980-etc-ssh-ssh_config")) };
    let after = Laid { hidden: vec![socket.clone()], ssh_config: Some(PathBuf::from("/nix/store/zhf1-etc-ssh-ssh_config")) };
    assert!(!before.serves(&after));
    assert!(before.serves(&Laid { hidden: vec![socket], ssh_config: before.ssh_config.clone() }));
}

#[test]
fn an_outside_child() {
    let Some(marker) = std::env::var_os(OUTSIDE_MARKER) else {
        return;
    };
    let _ = super::outside("/bin/sh", &["-c".to_owned(), format!("touch {}", marker.to_string_lossy())]);
    std::process::exit(126);
}

#[test]
fn hidden_sockets_without_the_mark_never_run_an_outside_command_in_place() {
    let scratch = tempfile::tempdir().unwrap();
    let marker = scratch.path().join("ran-in-place");
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "worker_sandbox::tests::an_outside_child", "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("XDG_RUNTIME_DIR", scratch.path())
        .env(super::SOCKETS_HIDDEN, "1")
        .env(OUTSIDE_MARKER, &marker)
        .output()
        .unwrap();
    assert!(!marker.exists(), "--outside ran its command in place under hidden sockets, so `capacity host run` exec'd itself in a loop at a full core: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn a_leased_caller_s_brokered_run_starts_inside_its_lease_scope_north_1036() {
    let north = Path::new("/n/north");
    let arguments = vec!["depbox".to_owned(), "run".to_owned(), "--".to_owned(), "cargo".to_owned(), "clippy".to_owned()];
    let leased = "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/agent-capacity.slice/agent-capacity-44328cc608bb40a3880b26c9e0d73a21.scope\n";
    let started = super::broker::brokered(north, arguments.clone(), Some(leased));
    let words: Vec<String> = started.get_args().map(|word| word.to_string_lossy().into_owned()).collect();
    assert_eq!(started.get_program(), "/bin/sh", "on gex45 on 2026-11-10 a leased land check's clippy ran in the broker's own service, so its lease read 0.05 cores while its compiles held 7");
    assert_eq!(words[2..], ["/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/agent.slice/agent-capacity.slice/agent-capacity-44328cc608bb40a3880b26c9e0d73a21.scope/cgroup.procs", "/n/north", "depbox", "run", "--", "cargo", "clippy"]);
    let open = super::broker::brokered(north, arguments, Some(OPEN_SCOPE));
    assert_eq!(open.get_program(), "/n/north");
}

#[test]
fn a_draining_broker_holding_the_unit_name_does_not_stop_a_new_broker_from_starting() {
    let directory = std::env::temp_dir().join(format!("north-1089-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let socket = directory.join("broker.sock");
    let listeners = std::cell::RefCell::new(Vec::new());
    let tried = std::cell::RefCell::new(Vec::new());
    super::broker::ensure_listening(&socket, |unit| {
        tried.borrow_mut().push(unit.to_owned());
        if unit == "north-worker-outside" {
            return false;
        }
        listeners.borrow_mut().push(std::os::unix::net::UnixListener::bind(&socket).unwrap());
        true
    });
    assert!(std::os::unix::net::UnixStream::connect(&socket).is_ok(), "no broker listened after the unit name was refused; tried {:?}", tried.borrow());
    std::fs::remove_dir_all(&directory).unwrap();
}

const WRITE_HOME: &str = "NORTH_1068_WRITE_HOME";

#[test]
fn a_lane_only_write_child() {
    let Some(home) = std::env::var_os(WRITE_HOME) else {
        return;
    };
    let home = PathBuf::from(home);
    let context = Context { home: home.clone(), cwd: home.join("code/alpha/worktrees/lane"), uid: 0, tmpdir: None, cache: None, config: None, runtime: None, cargo_home: None, cargo_target: None, named_lanes: Vec::new(), named_projects: Vec::new(), runtimes: None };
    assert!(matches!(super::confine_to_lane(&context, &[]), Ok(RulesetStatus::FullyEnforced)), "Landlock does not enforce here");
    let writes = |path: &str| std::fs::OpenOptions::new().write(true).open(home.join(path)).is_ok();
    assert!(writes("code/alpha/worktrees/lane/file"), "a lane-only command cannot write its own lane");
    for refused in [".local/state/north/fleet/sessions/s1.json", "sys/fs/cgroup/agent.slice/north-session-lane-s1.scope/cgroup.procs", "code/beta/main/file"] {
        assert!(!writes(refused), "a lane-only command wrote {refused}");
    }
}

#[test]
fn a_lane_only_command_cannot_write_a_fleet_record_a_cgroup_procs_file_or_a_sibling_project() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("north-1068-write").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    for file in ["code/alpha/main/.git/HEAD", "code/alpha/worktrees/lane/file", ".local/state/north/fleet/sessions/s1.json", "sys/fs/cgroup/agent.slice/north-session-lane-s1.scope/cgroup.procs", "code/beta/main/file"] {
        std::fs::create_dir_all(home.join(file).parent().unwrap()).unwrap();
        std::fs::write(home.join(file), "x").unwrap();
    }
    let output = Command::new(std::env::current_exe().unwrap()).args(["--exact", "worker_sandbox::tests::a_lane_only_write_child", "--nocapture"]).env(WRITE_HOME, &home).output().unwrap();
    assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}

#[test]
fn a_brokered_safe_push_queues_in_its_caller_s_state_home_and_every_other_request_in_the_broker_s() {
    let sent = [("XDG_STATE_HOME", "/tmp/verify-north-x/state"), ("HOME", "/home/a/elsewhere")].map(|(name, value)| (name.to_owned(), value.to_owned()));
    let own = || [("XDG_STATE_HOME", "/home/a/.local/state"), ("XDG_CACHE_HOME", "/home/a/.cache"), ("HOME", "/home/a")].map(|(name, value)| (name.to_owned(), value.to_owned())).to_vec();
    let words = |text: &str| -> Vec<String> { text.split(' ').map(str::to_owned).collect() };
    let state = |homes: Vec<(String, String)>| homes.into_iter().filter(|(name, _)| name == "XDG_STATE_HOME" || name == "HOME").map(|(_, value)| value).collect::<Vec<_>>();
    assert_eq!(state(super::broker::request_homes(&words("safe-push --to main"), Some(OPEN_SCOPE), &sent, own())), ["/tmp/verify-north-x/state", "/home/a"], "a worker's brokered safe-push queued its ticket in the host's landing queue, so verify-north found none in its scratch state (north#1022)");
    assert_eq!(state(super::broker::request_homes(&words("depbox run -- cargo check"), Some(OPEN_SCOPE), &sent, own())), ["/home/a/.local/state", "/home/a"]);
    assert_eq!(state(super::broker::request_homes(&words("safe-push --to main"), Some(LANE_SCOPE), &sent, own())), ["/home/a/.local/state", "/home/a"]);
}

#[test]
fn merge_tree_leaves_main_clean() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("north-1068-merge-tree").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let main = home.join("code/alpha/main");
    std::fs::create_dir_all(home.join("code/alpha/worktrees")).unwrap();
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["config", "merge.keep.driver", "true"]);
    std::fs::write(main.join(".gitattributes"), "features.md merge=keep\n").unwrap();
    std::fs::write(main.join("features.md"), "base\n").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "base"]);
    git(&main, &["checkout", "-q", "-b", "side"]);
    std::fs::write(main.join("features.md"), "side\n").unwrap();
    git(&main, &["commit", "-q", "-am", "side"]);
    git(&main, &["checkout", "-q", "main"]);
    std::fs::write(main.join("features.md"), "main\n").unwrap();
    git(&main, &["commit", "-q", "-am", "main"]);

    let command = "git merge-tree --write-tree main side";
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "worker_sandbox::tests::a_confined_command_child", "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", &home)
        .env(COMMAND, command)
        .current_dir(&main)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let status = || String::from_utf8(Command::new("git").arg("-C").arg(&main).args(["status", "--porcelain"]).output().unwrap().stdout).unwrap();
    assert!(status().contains(".merge_file_"), "the domain no longer leaves git's temporaries, so the wrapper's sweep guards nothing: {}", status());
    let mut context = Context::from_env(command);
    context.home = home;
    context.cwd = main.clone();
    super::sweep::clear(&super::sweep::places(&context));
    assert_eq!(status(), "", "main is not clean after the command's sweep");
}
