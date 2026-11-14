use std::path::{Path, PathBuf};
use std::process::Command;

use super::peer::Peer;

use landlock::RulesetStatus;
use serde_json::{Value, json};

use super::{Context, Reader, confine, executable, readable, removable};

pub(super) const COMMAND: &str = "NORTH_706_LOCKS_COMMAND";
pub(super) const WRITES_CONFINED: &str = "NORTH_1116_WRITES_CONFINED";
const LOGINS_ONLY: &str = "NORTH_977_LOGINS_ONLY";
const HAND_OFF_TARGET: &str = "NORTH_975_HAND_OFF_TARGET";
const OUTSIDE_MARKER: &str = "NORTH_CAPACITY_LOOP_MARKER";

#[path = "offload_request_tests.rs"]
mod offload_request;
#[path = "record_request_tests.rs"]
mod record_request;
#[path = "withdraw_request_tests.rs"]
mod withdraw_request;

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
    let writable = roots.as_deref().filter(|_| std::env::var_os(WRITES_CONFINED).is_some()).map(|roots| super::writable(&context, roots));
    assert!(matches!(confine(roots.as_deref(), &readable(&context, Reader::of(roots.is_some())), Some(&executable(&context)), writable.as_deref(), &[]), Ok(RulesetStatus::FullyEnforced)), "Landlock does not enforce here");
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
fn no_agent_command_reads_the_operators_gh_login_and_no_worker_command_reads_a_key_a_linked_secret_or_the_agent_socket_north_969_north_977_south_118() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("north-969-gh-login").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let gh = home.join(".config/gh");
    let (ssh, secrets) = (home.join(".ssh"), home.join("secrets"));
    for directory in [&gh, &ssh, &secrets] {
        std::fs::create_dir_all(directory).unwrap();
    }
    std::fs::write(gh.join("hosts.yml"), "github.com:\n    oauth_token: personal\n").unwrap();
    std::fs::write(gh.join("config.yml"), "git_protocol: https\n").unwrap();
    std::os::unix::fs::symlink(gh.join("hosts.yml"), home.join("hosts-link.yml")).unwrap();
    std::fs::write(ssh.join("made-for-the-test"), "not a key\n").unwrap();
    for name in ["linked", "agent", "beside"] {
        std::fs::write(secrets.join(name), "not a secret\n").unwrap();
    }
    std::os::unix::fs::symlink(secrets.join("linked"), ssh.join("peer")).unwrap();
    let logins = "! cat ~/.config/gh/hosts.yml && ! cat ~/hosts-link.yml && ! ln ~/.config/gh/hosts.yml /tmp/north-969-leak-$$ && cat ~/.config/gh/config.yml && cat ~/secrets/beside";
    let trusted = ["~/.ssh/made-for-the-test", "~/.ssh/peer", "~/secrets/linked", "~/secrets/agent"];
    for logins_only in [false, true] {
        let refused = if logins_only { "" } else { "! " };
        let command = std::iter::once(logins.to_owned()).chain(trusted.iter().map(|path| format!("{refused}cat {path}"))).collect::<Vec<_>>().join(" && ");
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.args(["--exact", "worker_sandbox::tests::a_confined_command_child", "--nocapture"]).env_clear().env("PATH", std::env::var_os("PATH").unwrap()).env("HOME", &home).env("SSH_AUTH_SOCK", secrets.join("agent")).env(COMMAND, &command).current_dir(&home);
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
    if offload_request::played() {
        return;
    }
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    let words = |text: &str| -> Vec<String> { text.split(' ').map(str::to_owned).collect() };
    let scratch = tempfile::tempdir().unwrap();
    let master = scratch.path().join(format!("{}0a1b", north_base::foundation::offload::MASTER_SOCKET));
    let _listening = UnixListener::bind(&master).unwrap();
    let host = scratch.path().join(format!("{}0a1b", north_base::platform::ssh::HOST_MASTER));
    let _riding = UnixListener::bind(&host).unwrap();
    let taker = scratch.path().join("north-taker-0a1b");
    let _taking = UnixListener::bind(&taker).unwrap();
    std::fs::create_dir_all(scratch.path().join("north-links")).unwrap();
    std::fs::create_dir_all(scratch.path().join("north-spares")).unwrap();
    let (link, spare) = (scratch.path().join("north-links/tom_gex45"), scratch.path().join("north-spares/tom_gex45.sock"));
    let (_linked, _spared) = (UnixListener::bind(&link).unwrap(), UnixListener::bind(&spare).unwrap());
    let mut hidden = super::sockets::hidden(scratch.path(), &scratch.path().join("tmux-0"), &[]);
    hidden.sort();
    let mut masters = vec![master, host, taker, link, spare];
    masters.sort();
    assert_eq!(hidden, masters, "an offload, host, taker or fleet-link master or a spare shell's holder is reachable from a worker although its name begins as north's own sockets do");
    let declared = |host: &str| (host == "gex45").then(|| format!("tom@{host}")).ok_or_else(|| format!("{host} is not a host this one reaches"));
    let run = "capacity offload run --host gex45 --base aaa --head bbb --timeout 60 -- true";
    for allowed in [run, "capacity offload run --background --host gex45 --base aaa --head bbb --timeout 60 -- true", "capacity offload free --host gex45", "capacity offload hosts", "capacity offload slots"] {
        assert!(super::broker::permitted(&words(allowed)), "{allowed}");
        assert_eq!(super::broker::offload_refusal(&words(allowed), false, declared), None, "{allowed}");
    }
    let undeclared = super::broker::offload_refusal(&words("capacity offload run --host elsewhere --base aaa --head bbb --timeout 60 -- true"), false, declared);
    assert!(undeclared.as_deref().is_some_and(|why| why.contains("elsewhere")), "a host this one does not reach: {undeclared:?}");
    assert!(super::broker::offload_refusal(&words("capacity offload free --host here"), false, declared).is_some(), "this host's own name, whose run stays in the caller's domain");
    assert!(super::broker::offload_refusal(&words(run), true, declared).is_some(), "a lane-only session");
    let north = Path::new("/nonexistent/north");
    let asked_of = |older: bool| {
        let (client, server) = UnixStream::pair().unwrap();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                if older {
                    let mut heard = [0u8; 4096];
                    let _ = (&server).read(&mut heard).unwrap();
                    (&server).write_all(&[126]).unwrap();
                } else {
                    super::broker::answer(north, &server, &Peer::from(None), &super::lane_grant::Host::own()).unwrap();
                }
            });
            super::broker::knows(&client, &words(run))
        })
    };
    let mut brokers = [true, true, false].into_iter();
    assert!(super::broker::await_knowing(|| asked_of(brokers.next().unwrap()), std::time::Duration::from_secs(5)), "a request the replaced build's broker has never heard waits for the broker that runs it");
    assert_eq!(brokers.next(), None, "it asked the older broker twice and stopped at the first that runs the request");
    assert!(!super::broker::await_knowing(|| asked_of(true), std::time::Duration::ZERO), "past the wait it is not sent to a broker that would refuse it");
    for refused in ["sh -c id", "depbox run", "depbox exec -- cargo", "lane exec x", "--worker-sandbox -- sh", "capacity host run --class moderate", "capacity host run --class moderate --", "capacity host status --json", "capacity host reserve --class heavy", "capacity offload reap --owner land", "capacity offload take --once", "capacity offload choose --any --needs /etc/shadow", "capacity offload run --host gex45 -o ProxyCommand=id --base aaa --head bbb --timeout 60 -- true", "capacity offload run --host gex45 --base --upload-pack=id --head bbb --timeout 60 -- true", "capacity offload run --host gex45 --base aaa --head bbb --timeout 60 -- true id", "capacity offload free --host gex45 -F /tmp/config", "gate answer 3 1", "gate list"] {
        assert!(!super::broker::permitted(&words(refused)), "{refused}");
    }
    for allowed in ["depbox run -- cargo check", "depbox fetch -- cargo fetch", "lane unlock x", "lane stray y", "lane remove z", "lane sync /c/main", "home move plan.tsv", "--worker-sandbox --built -- ./t", "--worker-sandbox --lane -- /bin/sh -c true", "capacity host run --class moderate --owner o --timeout-seconds 9 -- cargo check", "capacity host status", "capacity host backlog", "fleet need laptop the box times a build there", "gate open --question q --option a --option b --recommend 1 --ask-once cost"] {
        assert!(super::broker::permitted(&words(allowed)), "{allowed}");
    }
    let sent = [("LD_PRELOAD", "/tmp/x.so"), ("GIT_SSH_COMMAND", "sh"), ("GIT_CONFIG_COUNT", "9"), ("NORTH_WORKER_SANDBOXED", "1"), ("GIT_SSL_CAINFO", "/ca"), ("GIT_AUTHOR_NAME", "a"), ("NORTH_SOCKETS_HIDDEN", "1"), ("NORTH_WORKER_CONFINED", "1"), ("AGENT_CAPACITY_DB", "/home/a/forged.db"), ("NORTH_WORKER_REMOVAL_ROOTS", "[\"/\"]"), ("CARGO_HOME", "/c"), ("PATH", "bin:/usr/bin::/opt/x"), ("XDG_RUNTIME_DIR", "/forged"), ("XDG_STATE_HOME", "/tmp/north-opencode-1/state"), ("XDG_CACHE_HOME", "/tmp/north-opencode-1/cache"), ("XDG_DATA_HOME", "/tmp/north-opencode-1/data"), ("XDG_CONFIG_HOME", "/tmp/north-opencode-1/config"), ("HOME", "/home/a/elsewhere"), ("NORTH_OFFLOAD_SUBJECT", "/home/a/notes"), ("NORTH_OFFLOAD_OWNER", "north-land"), ("NORTH_LAND_RED_FIX", "1"), ("NORTH_OFFLOAD_LANDER_SLOT", "1"), ("NORTH_OFFLOAD_HOLD", "gex45 code-p-main-1 abc"), (north_providers::providers::opencode::KEY_ENV, "go-key"), (north_providers::providers::pi::KEY_ENV, "go-key")];
    let homes = [("XDG_STATE_HOME", "/home/a/.local/state"), ("XDG_CACHE_HOME", "/home/a/.cache"), ("XDG_DATA_HOME", "/home/a/.local/share"), ("XDG_CONFIG_HOME", "/home/a/.config"), ("HOME", "/home/a")].map(|(name, value)| (name.to_owned(), value.to_owned()));
    let sent_pairs = || sent.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect::<Vec<_>>();
    let environment = super::request_env::environment_for(&words("capacity host run --class heavy --owner o -- cargo test"), sent_pairs(), &Peer::from(Some(OPEN_SCOPE)), Path::new("/run/user/1000"), &homes, &[]);
    let own = [("PATH", "/usr/bin"), ("AGENT_CAPACITY_DB", "/home/a/.local/state/agents/machine-capacity/capacity.db"), ("NORTH_WORKER_CONFINED", "1")].map(|(name, value)| (name.to_owned(), value.to_owned()));
    let read = super::request_env::environment_for(&words("capacity host status"), sent_pairs(), &Peer::from(Some(OPEN_SCOPE)), Path::new("/run/user/1000"), &homes, &own);
    let read_get = |name: &str| read.iter().filter(|(own, _)| own == name).map(|(_, value)| value.as_str()).collect::<Vec<_>>();
    assert_eq!((read_get("XDG_RUNTIME_DIR"), read_get("AGENT_CAPACITY_DB"), read_get("GIT_AUTHOR_NAME"), read_get("NORTH_WORKER_CONFINED"), read_get("HOME")), (vec!["/run/user/1000"], vec!["/home/a/.local/state/agents/machine-capacity/capacity.db"], vec![], vec![], vec!["/home/a"]), "a worker's brokered status read the admission state where the client's environment pointed it, unconfined");
    let get = |name: &str| environment.iter().find(|(own, _)| own == name).map(|(_, value)| value.as_str());
    assert_eq!((get("LD_PRELOAD"), get("GIT_SSH_COMMAND"), get("NORTH_WORKER_SANDBOXED"), get("GIT_SSL_CAINFO"), get("CARGO_HOME"), get("PATH")), (None, None, None, Some("/ca"), Some("/c"), Some("/usr/bin:/opt/x")));
    assert_ne!(get("GIT_CONFIG_COUNT"), Some("9"));
    assert_eq!((get("NORTH_OFFLOAD_SUBJECT"), get("NORTH_OFFLOAD_OWNER"), get("NORTH_LAND_RED_FIX"), get("NORTH_OFFLOAD_LANDER_SLOT"), get("NORTH_OFFLOAD_HOLD")), (None, None, None, None, None), "a worker's request named the file admission records or claimed the lander's owner, red fix, slot or held slot");
    assert_eq!((get("GIT_AUTHOR_NAME"), get("NORTH_SOCKETS_HIDDEN"), get("NORTH_WORKER_REMOVAL_ROOTS")), (Some("a"), None, None));
    assert_eq!((get(north_providers::providers::opencode::KEY_ENV), get(north_providers::providers::pi::KEY_ENV)), (None, None), "a brokered session command read the paid Go key from the opencode or pi client's environment");
    assert_eq!(get("NORTH_WORKER_CONFINED"), None, "a brokered capacity run kept the confined mark, so it asked the broker again instead of taking its lease");
    assert_eq!(environment.iter().filter(|(name, _)| name == "XDG_RUNTIME_DIR").map(|(_, value)| value.as_str()).collect::<Vec<_>>(), ["/run/user/1000"], "an outside command took the client's runtime directory, so an OpenCode worker that clears it got no user manager and no capacity lease (north#1023)");
    assert_eq!(environment.iter().filter(|(name, _)| name.starts_with("XDG_") && name != "XDG_RUNTIME_DIR").map(|(name, value)| (name.as_str(), value.as_str())).collect::<Vec<_>>(), [("XDG_STATE_HOME", "/home/a/.local/state"), ("XDG_CACHE_HOME", "/home/a/.cache"), ("XDG_DATA_HOME", "/home/a/.local/share"), ("XDG_CONFIG_HOME", "/home/a/.config")], "a brokered build took the client's scratch homes, so its capacity admissions landed outside the host's decision ledger (north#1030)");
    assert_eq!(environment.iter().filter(|(name, _)| name == "HOME").map(|(_, value)| value.as_str()).collect::<Vec<_>>(), ["/home/a"], "a brokered command took the client's HOME instead of the broker's (north#992)");
    let north = Path::new("/n/north");
    let wrapped = super::broker::confined(north, words("capacity host run --class heavy --owner o -- cargo test -p x"));
    assert_eq!(wrapped.join(" "), "capacity host run --class heavy --owner o -- /n/north --worker-sandbox --program -- cargo test -p x");
    offload_request::an_offload_request_s_process_takes_nothing_of_the_worker_s_but_its_typed_words();
    super::eval_unit_request::tests::a_listed_lead_starts_reads_and_stops_only_the_finish_proof_unit_and_a_request_outside_the_closed_list_starts_nothing();
}

#[test]
fn a_worker_lane_writes_its_slot_and_finish_records_through_the_broker() {
    if record_request::played() {
        return;
    }
    let defaults = super::request_env::homes(Some(PathBuf::from("/home/a")), |_| None);
    assert_eq!(defaults, [("XDG_STATE_HOME", "/home/a/.local/state"), ("XDG_CACHE_HOME", "/home/a/.cache"), ("XDG_DATA_HOME", "/home/a/.local/share"), ("XDG_CONFIG_HOME", "/home/a/.config")].map(|(name, value)| (name.to_owned(), value.to_owned())).to_vec());
    let named = super::request_env::homes(None, |name| (name == "XDG_STATE_HOME").then(|| PathBuf::from("/srv/state")));
    assert_eq!(named.iter().find(|(name, _)| name == "XDG_STATE_HOME").map(|(_, value)| value.as_str()), Some("/srv/state"), "a home the host set was replaced by a default");
    record_request::a_worker_lane_writes_its_slot_and_finish_records_through_the_broker();
}

const OPEN_SCOPE: &str = "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-session-77c49ab1-cccb-4a4d-8623-d56cb8891ace.scope\n";
const LANE_SCOPE: &str = "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-session-lane-77c49ab1-cccb-4a4d-8623-d56cb8891ace.scope\n";

#[test]
fn a_lane_only_caller_without_its_marker_still_gets_lane_reads() {
    let sent = || vec![("PATH".to_owned(), "/usr/bin".to_owned()), ("HOME".to_owned(), "/home/a".to_owned())];
    let homes = [("XDG_STATE_HOME".to_owned(), "/home/a/.local/state".to_owned()), ("XDG_CACHE_HOME".to_owned(), "/home/a/.cache".to_owned()), ("XDG_DATA_HOME".to_owned(), "/home/a/.local/share".to_owned()), ("XDG_CONFIG_HOME".to_owned(), "/home/a/.config".to_owned())];
    let marker = |environment: &[(String, String)]| environment.iter().find(|(name, _)| name == super::LANE_ONLY).map(|(_, value)| value.clone());
    assert_eq!(marker(&super::request_env::environment(sent(), &Peer::from(Some(LANE_SCOPE)), Path::new("/run/user/1000"), &homes)), Some("1".to_owned()), "a lane-only scope's caller without its marker got wide reads");
    assert_eq!(marker(&super::request_env::environment(sent(), &Peer::from(None), Path::new("/run/user/1000"), &homes)), Some("1".to_owned()), "a caller whose scope cannot be read got wide reads");
    assert_eq!(marker(&super::request_env::environment(sent(), &Peer::from(Some(OPEN_SCOPE)), Path::new("/run/user/1000"), &homes)), None);
    let forged = sent().into_iter().chain([(super::LANE_ONLY.to_owned(), "1".to_owned())]).collect();
    assert_eq!(marker(&super::request_env::environment(forged, &Peer::from(Some(OPEN_SCOPE)), Path::new("/run/user/1000"), &homes)), Some("1".to_owned()), "a caller may narrow itself");
}

#[test]
fn a_worker_may_not_remove_the_transcript_stores_or_the_ledger_beside_its_scratch_north_1041_nor_write_the_private_blob_cache() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let root = tempfile::Builder::new().prefix("north-1041-transcripts").tempdir_in(outside_tmp).unwrap();
    let home = root.path().canonicalize().unwrap();
    let state = home.join(".local/state");
    let main = home.join("code/north/main");
    for directory in [state.join("threads"), state.join("convo"), state.join("north/profiles"), state.join("north/fleet"), state.join("north/land"), main.clone()] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let context = Context { home, cwd: main, uid: 0, user_runtime: root.path().join("run"), tmpdir: None, cache: None, config: None, runtime: None, cargo_home: None, cargo_target: None, named_lanes: Vec::new(), named_projects: Vec::new(), runtimes: None, session: None, agent_socket: None, offload_slot: None, land_check: false };
    let roots = removable(&context);
    for store in ["threads", "convo", "north/profiles", "north/fleet"] {
        let store = state.join(store).canonicalize().unwrap();
        assert!(!roots.iter().any(|root| store.starts_with(root) || root.starts_with(&store)), "a worker may remove the transcript store {}", store.display());
    }
    let scratch = state.join("north/land").canonicalize().unwrap();
    assert!(roots.iter().any(|root| root == &scratch), "a worker may no longer remove its own lock files under {}", scratch.display());
    let cache = state.join("north/private-blobs");
    std::fs::create_dir_all(&cache).unwrap();
    let cache = cache.canonicalize().unwrap();
    for land_check in [false, true] {
        let context = Context { home: context.home.clone(), cwd: context.cwd.clone(), uid: 0, user_runtime: context.user_runtime.clone(), tmpdir: None, cache: None, config: None, runtime: None, cargo_home: None, cargo_target: None, named_lanes: Vec::new(), named_projects: Vec::new(), runtimes: None, session: None, agent_socket: None, offload_slot: None, land_check };
        let (roots, written) = (removable(&context), super::writable(&context, &removable(&context)));
        assert!(!roots.iter().chain(&written).any(|path| cache.starts_with(path) || path.starts_with(&cache)), "a worker may write the private-blob cache {}, which the private-file refusal trusts", cache.display());
    }
    let written = super::writable(&context, &roots);
    let records = context.home.join(north_base::foundation::offload::SLOT_RECORDS);
    std::fs::create_dir_all(&records).unwrap();
    assert!(!roots.iter().chain(&written).any(|path| records.starts_with(path) || path.starts_with(&records)), "a worker may write or remove a build slot's record under {}, which tells the broker the project a request from that slot is served as", records.display());
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
    let context = Context { home: home.clone(), cwd: home.join("code/north/main"), uid: 0, user_runtime: home.join("run"), tmpdir: None, cache: None, config: None, runtime: None, cargo_home: None, cargo_target: None, named_lanes: Vec::new(), named_projects: Vec::new(), runtimes: Some(runtimes.clone()), session: None, agent_socket: None, offload_slot: None, land_check: false };
    let hidden: Vec<PathBuf> = super::lane_only::hidden(&context, &[]).iter().map(|path| path.canonicalize().unwrap_or_else(|_| path.clone())).collect();
    assert!(!hidden.iter().any(|path| runtimes.join("a/tree/bin").starts_with(path)), "the north runtime is hidden: {hidden:?}");
    assert!(hidden.contains(&state.join("profiles")), "the agent state beside the runtime is readable: {hidden:?}");
}

#[test]
fn namespaces_laid_before_a_nixos_activation_moved_ssh_config_are_not_reused_north_1083() {
    use super::broker::Laid;
    let socket = PathBuf::from("/run/user/1000/bus");
    let before = Laid { hidden: vec![socket.clone()], ssh_config: Some(PathBuf::from("/nix/store/d980-etc-ssh-ssh_config")), inodes: Vec::new() };
    let after = Laid { hidden: vec![socket.clone()], ssh_config: Some(PathBuf::from("/nix/store/zhf1-etc-ssh-ssh_config")), inodes: Vec::new() };
    assert!(!before.serves(&after));
    assert!(before.serves(&Laid { hidden: vec![socket], ssh_config: before.ssh_config.clone(), inodes: Vec::new() }));
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
    let started = super::broker::brokered(north, arguments.clone(), &Peer::from(Some(leased)));
    let words: Vec<String> = started.get_args().map(|word| word.to_string_lossy().into_owned()).collect();
    assert_eq!(started.get_program(), "/bin/sh", "on gex45 on 2026-11-10 a leased land check's clippy ran in the broker's own service, so its lease read 0.05 cores while its compiles held 7");
    assert_eq!(words[2..], ["/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/agent.slice/agent-capacity.slice/agent-capacity-44328cc608bb40a3880b26c9e0d73a21.scope/cgroup.procs", "/n/north", "depbox", "run", "--", "cargo", "clippy"]);
    let open = super::broker::brokered(north, arguments, &Peer::from(Some(OPEN_SCOPE)));
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
const WRITE_DOMAIN: &str = "NORTH_1041_WRITE_DOMAIN";
const ADMITTED: [&str; 3] = ["run/agent-capacity-v1/leases/held.json", "run/agent-capacity-v1/lifecycle.db", "run/agent-capacity-v1/lifecycle.db-shm"];

#[test]
fn a_confined_write_child() {
    let Some(home) = std::env::var_os(WRITE_HOME) else {
        return;
    };
    let home = PathBuf::from(home);
    let lane_only = std::env::var_os(WRITE_DOMAIN).is_some_and(|domain| domain == "lane");
    let context = Context { home: home.clone(), cwd: home.join("code/alpha/worktrees/lane"), uid: 0, user_runtime: home.join("run"), tmpdir: None, cache: None, config: None, runtime: None, cargo_home: None, cargo_target: None, named_lanes: Vec::new(), named_projects: Vec::new(), runtimes: None, session: None, agent_socket: None, offload_slot: None, land_check: false };
    let confined = if lane_only {
        super::confine_to_lane(&context, &[])
    } else {
        let roots = removable(&context);
        confine(Some(&roots), &readable(&context, Reader::Worker), Some(&executable(&context)), Some(&super::writable(&context, &roots)), &[])
    };
    assert!(matches!(confined, Ok(RulesetStatus::FullyEnforced)), "Landlock does not enforce here");
    let writes = |path: &str| std::fs::OpenOptions::new().write(true).open(home.join(path)).is_ok();
    let creates = |path: &str| std::fs::OpenOptions::new().write(true).create_new(true).open(home.join(path)).is_ok();
    assert!(writes("code/alpha/worktrees/lane/file"), "a confined command cannot write its own lane");
    for refused in ADMITTED {
        assert!(!writes(refused) && !creates(refused), "a confined command wrote the capacity admission state's {refused} in place, past the broker");
    }
    assert!(!creates("run/agent-capacity-v1/leases/forged.json"), "a confined command forged a capacity lease");
    assert!(creates("run/agent-capacity-v1/sccache/lease.sock") && creates("run/north-gh-budget/worker") && creates("run/kept/file"), "a confined command lost its lease's compiler-cache socket directory or the rest of its runtime directory");
    if lane_only {
        for refused in [".local/state/north/fleet/sessions/s1.json", "sys/fs/cgroup/agent.slice/north-session-lane-s1.scope/cgroup.procs", "code/beta/main/file"] {
            assert!(!writes(refused), "a lane-only command wrote {refused}");
        }
    } else {
        let ledger = north_base::lifecycle::Store::read_only(&home.join(ADMITTED[1])).and_then(|store| store.faults(north_base::foundation::clock::Timestamp::now()));
        assert!(ledger.is_ok(), "a confined reader cannot read the capacity lifecycle ledger: {:?}", ledger.err());
    }
}

#[test]
fn a_worker_command_cannot_write_the_capacity_admission_state_nor_a_lane_only_one_a_fleet_record_a_cgroup_procs_file_or_a_sibling_project() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    for domain in ["lane", "worker"] {
        let scratch = tempfile::Builder::new().prefix("north-1068-write").tempdir_in(&outside_tmp).unwrap();
        let home = scratch.path().canonicalize().unwrap();
        for file in ["code/alpha/main/.git/HEAD", "code/alpha/worktrees/lane/file", ".local/state/north/fleet/sessions/s1.json", "sys/fs/cgroup/agent.slice/north-session-lane-s1.scope/cgroup.procs", "code/beta/main/file", "run/kept/.keep", ADMITTED[0]] {
            std::fs::create_dir_all(home.join(file).parent().unwrap()).unwrap();
            std::fs::write(home.join(file), "x").unwrap();
        }
        drop(north_base::lifecycle::Store::open(&home.join(ADMITTED[1])).unwrap());
        let output = Command::new(std::env::current_exe().unwrap()).args(["--exact", "worker_sandbox::tests::a_confined_write_child", "--nocapture"]).env(WRITE_HOME, &home).env(WRITE_DOMAIN, domain).output().unwrap();
        assert!(output.status.success(), "{domain}: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert_eq!(std::fs::read_dir(home.join("run/agent-capacity-v1/leases")).unwrap().count(), 1, "{domain}: a confined command changed the admission state's leases");
    }
}

#[test]
fn a_brokered_safe_push_queues_in_its_caller_s_state_home_and_every_other_request_in_the_broker_s() {
    let sent = [("XDG_STATE_HOME", "/tmp/verify-north-x/state"), ("HOME", "/home/a/elsewhere")].map(|(name, value)| (name.to_owned(), value.to_owned()));
    let own = || [("XDG_STATE_HOME", "/home/a/.local/state"), ("XDG_CACHE_HOME", "/home/a/.cache"), ("HOME", "/home/a")].map(|(name, value)| (name.to_owned(), value.to_owned())).to_vec();
    let words = |text: &str| -> Vec<String> { text.split(' ').map(str::to_owned).collect() };
    let state = |homes: Vec<(String, String)>| homes.into_iter().filter(|(name, _)| name == "XDG_STATE_HOME" || name == "HOME").map(|(_, value)| value).collect::<Vec<_>>();
    assert_eq!(state(super::request_env::request_homes(&words("safe-push --to main"), &Peer::from(Some(OPEN_SCOPE)), &sent, own())), ["/tmp/verify-north-x/state", "/home/a"], "a worker's brokered safe-push queued its ticket in the host's landing queue, so verify-north found none in its scratch state (north#1022)");
    assert_eq!(state(super::request_env::request_homes(&words("depbox run -- cargo check"), &Peer::from(Some(OPEN_SCOPE)), &sent, own())), ["/home/a/.local/state", "/home/a"]);
    assert_eq!(state(super::request_env::request_homes(&words("safe-push --to main"), &Peer::from(Some(LANE_SCOPE)), &sent, own())), ["/home/a/.local/state", "/home/a"]);
    let forged: Vec<(String, String)> = sent.iter().cloned().chain([("NORTH_VERDICT_CACHE_DIR", "/tmp/forged-passes"), ("AGENTS_ORCHESTRATION", "/tmp/forged.toml"), ("PATH", "/tmp/fakes:/usr/bin"), ("NORTH_LAND_NORTH", "/tmp/fakes/north"), ("SAFE_PUSH_LAND_OFFLOAD_HOST", "elsewhere"), ("RUSTC_WRAPPER", "/tmp/fakes/wrapper"), ("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER", "true"), ("RUSTUP_TOOLCHAIN", "forged"), ("CC_x86_64_unknown_linux_gnu", "/tmp/fakes/cc"), ("LIBRARY_PATH", "/tmp/fakes/lib")].map(|(name, value)| (name.to_owned(), value.to_owned()))).collect();
    let homes = super::request_env::request_homes(&words("safe-push --to main"), &Peer::from(Some(OPEN_SCOPE)), &forged, own());
    let broker = [("PATH", "/run/current-system/sw/bin"), ("RUSTC_WRAPPER", "sccache")].map(|(name, value)| (name.to_owned(), value.to_owned()));
    let chosen = super::request_env::landing_choices(&words("safe-push --to main"), Path::new("/home/a"), &broker, Path::new("/opt/broker/north"));
    let environment = super::request_env::landing(super::request_env::environment(forged.clone(), &Peer::from(Some(OPEN_SCOPE)), Path::new("/run/user/1000"), &homes), chosen);
    let named = |wanted: &str| environment.iter().filter(|(name, _)| name == wanted).map(|(_, value)| value.as_str()).collect::<Vec<_>>();
    assert_eq!((named("NORTH_VERDICT_CACHE_DIR"), named("AGENTS_ORCHESTRATION")), (vec!["/home/a/.local/state/north/verdict-cache"], vec![]), "a brokered safe-push reused passes from a verdict store or read a policy file its caller named");
    assert_eq!((named("PATH"), named("NORTH_LAND_NORTH"), named("SAFE_PUSH_LAND_OFFLOAD_HOST")), (vec!["/run/current-system/sw/bin"], vec!["/opt/broker/north"], vec![]), "a brokered safe-push ran programs or placed its checks before queueing where its caller named");
    assert_eq!((named("RUSTC_WRAPPER"), named("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER")), (vec!["sccache"], vec![]), "a brokered safe-push built or ran its checks before queueing with a compiler wrapper or test runner its caller named");
    assert_eq!((named("RUSTUP_TOOLCHAIN"), named("CC_x86_64_unknown_linux_gnu"), named("LIBRARY_PATH")), (vec![], vec![], vec![]), "a brokered safe-push built its checks before queueing on a toolchain, target compiler or library path its caller named");
    let build = super::request_env::landing(super::request_env::environment(forged, &Peer::from(Some(OPEN_SCOPE)), Path::new("/run/user/1000"), &homes), super::request_env::landing_choices(&words("depbox run -- cargo check"), Path::new("/home/a"), &broker, Path::new("/opt/broker/north")));
    assert_eq!(build.iter().filter(|(name, _)| name == "PATH").map(|(_, value)| value.as_str()).collect::<Vec<_>>(), ["/tmp/fakes:/usr/bin"]);
}

#[test]
fn review_pass_records_only_a_reviewer_session_s_accept_and_a_forged_session_never_reaches_the_enqueue_north_1287() {
    use north_base::platform::review_accept::Accepts;

    use crate::runtime::review_accept::Accept;

    const PATCH: &str = "0123456789abcdef0123456789abcdef01234567";
    let unit = |name: &str| format!("north-session-{name}.scope");
    let cgroup = |name: &str| format!("0::/user.slice/user-1000.slice/user@1000.service/agent.slice/{}\n", unit(name));
    let root = tempfile::tempdir().unwrap();
    let fleet = root.path().join("fleet");
    for (name, role) in [("north-reviewer", "reviewer"), ("north-author", "worker")] {
        let directory = fleet.join(name);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("host.scope"), unit(name)).unwrap();
        std::fs::write(directory.join("session.json"), json!({"name": name, "role": role, "cwd": root.path()}).to_string()).unwrap();
    }
    let host = super::lane_grant::Host { fleet, home: root.path().join("home"), code: Vec::new(), environment: Vec::new(), cgroups: root.path().join("cgroup") };

    let store = Accepts(root.path().join("accepts"));
    let accept = Accept { patch_id: PATCH.to_owned(), withdraw: false };
    let caller = |name: &str| super::hold_request::caller(&Peer::from(Some(cgroup(name).as_str())), &host).unwrap();
    accept.serve(&store, &caller("north-reviewer"), 1).unwrap();
    assert_eq!(store.sessions(PATCH).unwrap().into_iter().collect::<Vec<_>>(), ["north-reviewer"]);
    assert!(accept.serve(&store, &caller("north-author"), 2).is_err(), "a worker-role session, the lane's author among them, records no review pass");
    assert_eq!(store.sessions(PATCH).unwrap().len(), 1);

    let words = |text: &str| -> Vec<String> { text.split(' ').map(str::to_owned).collect() };
    let forged: Vec<(String, String)> = [("NORTH_FLEET_SESSION", "north-reviewer"), ("NORTH_FLEET_SESSION_PROVED", "0")].map(|(name, value)| (name.to_owned(), value.to_owned())).to_vec();
    let enqueue = |scope: &str| {
        let asker = super::request_env::asking_session(&words("safe-push --to main"), &Peer::from(Some(scope)), &host).unwrap();
        super::request_env::speaking_as(super::request_env::environment_for(&words("safe-push --to main"), forged.clone(), &Peer::from(Some(scope)), Path::new("/run/user/1000"), &[], &[]), asker)
    };
    let named = |variables: &[(String, String)], wanted: &str| variables.iter().filter(|(name, _)| name == wanted).map(|(_, value)| value.clone()).collect::<Vec<_>>();
    let author = enqueue(&cgroup("north-author"));
    assert_eq!((named(&author, "NORTH_FLEET_SESSION"), named(&author, "NORTH_FLEET_SESSION_PROVED")), (vec!["north-author".to_owned()], vec!["1".to_owned()]), "a brokered safe-push enqueued under a session name its caller forged, not the one whose scope the broker read");
    let unscoped = enqueue("0::/user.slice/app.slice/tool.scope\n");
    assert_eq!((named(&unscoped, "NORTH_FLEET_SESSION"), named(&unscoped, "NORTH_FLEET_SESSION_PROVED")), (vec![String::new()], Vec::<String>::new()), "a brokered safe-push from a caller in no session's scope enqueued under the name it sent");
}

#[test]
fn a_coordinator_session_is_refused_the_commands_only_a_fleet_session_runs() {
    use crate::runtime::review_hold::{Ask, Holds};

    let checkout = tempfile::tempdir().unwrap();
    let git = |arguments: &[&str]| std::process::Command::new("git").current_dir(checkout.path()).args(arguments).output().unwrap();
    git(&["init", "-q"]);
    git(&["-c", "user.name=north", "-c", "user.email=north@invalid", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "held"]);
    let commit = String::from_utf8(git(&["rev-parse", "HEAD"]).stdout).unwrap().trim().to_owned();
    let unit = |name: &str| format!("north-session-{name}.scope");
    let cgroup = |name: &str| format!("0::/user.slice/user-1000.slice/user@1000.service/agent.slice/{}\n", unit(name));
    let root = tempfile::tempdir().unwrap();
    let fleet = root.path().join("fleet");
    for (name, role) in [("proxy-session", "proxy"), ("lead-session", "lead")] {
        let directory = fleet.join(name);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("host.scope"), unit(name)).unwrap();
        std::fs::write(directory.join("session.json"), json!({"name": name, "role": role, "cwd": root.path()}).to_string()).unwrap();
    }
    let host = super::lane_grant::Host { fleet, home: root.path().join("home"), code: Vec::new(), environment: Vec::new(), cgroups: root.path().join("cgroup") };
    let holds = Holds::of(&[("NORTH_VERDICT_CACHE_DIR".to_owned(), root.path().join("verdicts").display().to_string())], &host.home);
    let caller = |name: &str| super::hold_request::caller(&Peer::from(Some(cgroup(name).as_str())), &host).unwrap();
    let place = || Ask::Place { commit: commit.clone(), lane: "f-fix".to_owned(), host: None, why: "held for review".to_owned() };
    let lift = || Ask::Lift { commit: commit.clone(), why: "reviewed".to_owned() };
    let refusal = "proxy-session runs in the coordinator role, which neither places nor lifts a hold";

    let placed = holds.serve(place(), &caller("proxy-session"), 1, checkout.path());
    assert!(placed.as_ref().is_err_and(|why| why.contains(refusal)), "a coordinator-role fleet session placed a hold its client window cannot: {placed:?}");
    let placed = holds.serve(place(), &caller("lead-session"), 2, checkout.path());
    assert!(placed.as_ref().is_ok_and(|line| line.contains("lead-session holds")), "a lead lost its hold: {placed:?}");
    let lifted = holds.serve(lift(), &caller("proxy-session"), 3, checkout.path());
    assert!(lifted.as_ref().is_err_and(|why| why.contains(refusal)), "a coordinator-role fleet session lifted a hold its client window cannot: {lifted:?}");
    let lifted = holds.serve(lift(), &caller("lead-session"), 4, checkout.path());
    assert!(lifted.as_ref().is_ok_and(|line| line.contains("lead-session lifted the hold of lead-session")), "a lead lost its lift: {lifted:?}");
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

const RESCUE_MAIN: &str = "NORTH_1068_RESCUE_MAIN";

#[test]
fn a_confined_lane_rescue_child() {
    let Some(main) = std::env::var_os(RESCUE_MAIN) else {
        return;
    };
    let context = Context::from_env("north lane rescue");
    let roots = removable(&context);
    assert!(matches!(confine(Some(&roots), &readable(&context, Reader::Worker), Some(&executable(&context)), None, &[]), Ok(RulesetStatus::FullyEnforced)), "Landlock does not enforce here");
    let _ = north_lanes::lanes::wt_rescue(&[main.to_string_lossy().into_owned()]);
}

#[test]
fn lane_rescue_leaves_main_clean() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("north-1068-lane-rescue").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let main = home.join("code/alpha/main");
    std::fs::create_dir_all(home.join("code/alpha/worktrees")).unwrap();
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    std::fs::write(main.join("tracked.txt"), "base\n").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "base"]);
    std::fs::write(main.join("tracked.txt"), "changed\n").unwrap();
    std::fs::write(main.join(".merge_file_abc123"), "left by git\n").unwrap();

    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "worker_sandbox::tests::a_confined_lane_rescue_child", "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", &home)
        .env(RESCUE_MAIN, &main)
        .current_dir(&main)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let rescued = std::fs::read_dir(home.join("code/alpha/worktrees")).unwrap().flatten().filter(|lane| lane.file_name().to_string_lossy().starts_with("rescue-")).count();
    let status = String::from_utf8(Command::new("git").arg("-C").arg(&main).args(["status", "--porcelain"]).output().unwrap().stdout).unwrap();
    assert!(status.is_empty() || rescued == 0, "main is dirty ({status:?}) and a rescue lane was left: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}


const VERDICT: &str = "Done: landed 84f42d59023c6d974df27d3d08c240169dfabf35 on main";

#[test]
fn a_hand_in_call_that_prints_a_long_log_returns_a_short_result_ending_in_its_verdict_with_the_log_by_path() {
    let scratch = tempfile::tempdir().unwrap();
    let north = scratch.path().join("north");
    let noisy = format!("i=0\nwhile [ $i -lt 4000 ]; do printf '{{\"decision\":\"RESERVED\",\"lease\":\"%s\"}}\\n' \"$i\"; i=$((i+1)); done\nprintf 'safe-push: ✗ the refusal a worker must see\\n' >&2\nprintf '{VERDICT}\\n'\n");
    north_base::foundation::shim::write_script(&north, &noisy);
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "hand-in", "arguments": {"message": "Hand it in", "paths": ["/lane/file"]}}});
    let mut answer = Vec::new();
    super::tool_server::serve(&["hand-in".to_owned()], &north, format!("{request}\n").as_bytes(), &mut answer).unwrap();

    let answer: Value = serde_json::from_slice(&answer).unwrap();
    let text = answer["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.chars().count() <= 5_000, "the result holds {} characters: {}", text.chars().count(), &text[..text.len().min(400)]);
    assert_eq!(text.lines().last(), Some(VERDICT), "{text}");
    assert!(text.lines().next().is_some_and(|line| line.contains("the refusal a worker must see")), "{text}");
    let log = text.lines().find_map(|line| line.split_once(" is in ").map(|(_, path)| std::path::PathBuf::from(path))).unwrap_or_else(|| panic!("no log path: {text}"));
    let kept = std::fs::read_to_string(&log).unwrap();
    std::fs::remove_file(&log).unwrap();
    assert!(kept.lines().count() > 4_000 && kept.trim_end().ends_with(VERDICT), "the kept log is not the whole output: {} lines", kept.lines().count());
}


#[test]
fn a_command_the_kernel_cannot_confine_is_refused_unless_the_host_names_itself_without_landlock_north_1041() {
    if std::env::var_os("NORTH_TEST_LANDLOCK_CHILD").is_some() {
        super::report(Err("no Landlock".to_owned()));
        println!("RAN");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    for (declared, ran) in [("", false), ("landlock = false\n", true)] {
        let policy = directory.path().join("orchestration.toml");
        std::fs::write(&policy, format!("[hosts.probe]\n{declared}")).unwrap();
        let output = Command::new(std::env::current_exe().unwrap()).args(["worker_sandbox::tests::a_command_the_kernel_cannot_confine", "--nocapture"]).env("NORTH_TEST_LANDLOCK_CHILD", "1").env("RESIDENT_CAP_HOST", "probe").env("AGENTS_ORCHESTRATION", &policy).output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("running 1 test"), "the child test was not found: {stdout}");
        assert_eq!(stdout.contains("RAN"), ran, "{declared:?}: {stdout}");
        assert_eq!(output.status.code() == Some(126), !ran, "{declared:?}: {stdout}");
    }
}

const SERVE_PROBE: &str = "NORTH_RUNTIME_BROKER_SERVE_PROBE";

#[test]
fn a_broker_on_a_socket_of_its_own_follows_the_runtime_its_canary_judges_and_the_shared_one_follows_current() {
    if std::env::var_os(SERVE_PROBE).is_some() {
        let answer: super::broker::Answer = std::sync::Arc::new(|_: &Path, _: &std::os::unix::net::UnixStream, _: &Peer| -> Result<(), String> { Ok(()) });
        assert_ne!(super::broker::serve(Vec::new(), answer), std::process::ExitCode::SUCCESS);
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let runtimes = root.path().join("runtimes");
    let (current, judged) = ("a".repeat(40), "b".repeat(40));
    for sha in [&current, &judged] {
        std::fs::create_dir_all(runtimes.join(sha)).unwrap();
    }
    std::fs::write(runtimes.join(&judged).join("north"), "").unwrap();
    let host = root.path().join("current");
    std::os::unix::fs::symlink(runtimes.join(&current), &host).unwrap();
    let starter = root.path().join("judged");
    std::os::unix::fs::symlink(runtimes.join(&judged), &starter).unwrap();
    let executable = runtimes.join(&judged).join("north");
    let own = super::broker::follow_for(true, &starter, &host, Some(executable.clone())).replaced();
    assert_eq!(own, None, "a broker on its own socket was replaced by the host's current runtime while it serves its canary's judged one");
    let shared = super::broker::follow_for(false, &starter, &host, Some(executable)).replaced();
    assert_eq!(shared.as_deref(), Some(current.as_str()), "the shared broker must follow the host's current runtime");
    let blocked = root.path().join("blocked");
    std::fs::write(&blocked, "").unwrap();
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "worker_sandbox::tests::a_broker_on_a_socket_of_its_own_follows_the_runtime_its_canary_judges_and_the_shared_one_follows_current", "--nocapture"])
        .env(SERVE_PROBE, "1")
        .env(super::broker::SOCKET_ENV, blocked.join("broker.sock"))
        .output()
        .unwrap();
    let seen = format!("{}{}", String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
    assert!(child.status.success(), "a broker whose socket cannot be listened on must not exit as if it served: {seen}");
    assert!(seen.contains("cannot listen"), "{seen}");
}

#[test]
fn a_lead_with_no_task_is_confined_like_a_tasked_one() {
    use north_base::platform::session_scope::{PROXY_ROLE, confined_role};
    assert!(confined_role(Some("lead")), "a lead is confined whether or not it holds a task");
    assert!(confined_role(Some("worker")), "a worker is confined whether or not it holds a task");
    assert!(confined_role(Some("reviewer")), "a reviewer is confined whether or not it holds a task");
    assert!(!confined_role(Some(PROXY_ROLE)), "the proxy seat drives windows and units");
    assert!(!confined_role(None), "an adopted human window carries no launcher role");
}

const UNMARKED: &str = "NORTH_1485_UNMARKED";

#[test]
fn a_confined_command_that_unsets_its_mark_still_reads_confined() {
    if std::env::var_os(UNMARKED).is_some() {
        assert!(matches!(super::confine_removal(&[]), Ok(RulesetStatus::FullyEnforced)), "Landlock does not enforce here");
        std::process::exit(if super::confined() { 0 } else { 1 });
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "worker_sandbox::tests::a_confined_command_that_unsets_its_mark_still_reads_confined", "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env(UNMARKED, "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "a command in a Landlock domain without the sandbox mark read as unconfined: {}", String::from_utf8_lossy(&output.stderr));
}
