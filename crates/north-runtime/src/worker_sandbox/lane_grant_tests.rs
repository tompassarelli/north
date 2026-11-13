use std::fs::File;
use std::io::Read;
use std::os::fd::AsFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::foundation::shim::write_executable;
use north_base::platform::gh_shim;

use super::super::broker::{self, Request};
use super::Host;

const HELD: &str = "ghs_heldbythebrokeronlyAAAAAAAAAA";
const UNIT: &str = "north-session-lane-1056.scope";
const NAME: &str = "n1056";
const TASK: &str = "tompassarelli/north#1056";
const SOCKET_ENV: &str = "NORTH_LANE_GRANT_TEST_SOCKET";
const CLIENT: &str = "worker_sandbox::lane_grant::tests::lane_only_client";

struct Fixture {
    root: tempfile::TempDir,
    host: Host,
    north: PathBuf,
    lane: PathBuf,
    other: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let main = home.join("code/north/main");
        let lane = home.join("code/north/worktrees").join(NAME);
        let other = home.join("code/wisp/worktrees").join(NAME);
        let shim = home.join(".local/bin");
        let fleet = root.path().join("fleet");
        for directory in [&main, &home.join("code/wisp/main"), &shim, &fleet.join(NAME)] {
            std::fs::create_dir_all(directory).unwrap();
        }
        for (main, lane) in [(&main, &lane), (&home.join("code/wisp/main"), &other)] {
            git(main, &["init", "-q", "-b", "main"]);
            git(main, &["commit", "-q", "--allow-empty", "-m", "one"]);
            git(main, &["worktree", "add", "-q", lane.to_str().unwrap(), "-b", NAME]);
        }
        write_executable(&shim.join("gh"), format!("#!/bin/sh\n# {}\n", gh_shim::MARKER));
        std::fs::write(fleet.join(NAME).join("host.scope"), format!("{UNIT}\n")).unwrap();
        std::fs::write(fleet.join(NAME).join("session.json"), serde_json::json!({ "name": NAME, "cwd": main, "task_ref": TASK }).to_string()).unwrap();
        let north = root.path().join("north");
        let log = root.path().join("ran");
        write_executable(&north, format!("#!/bin/sh\n[ -z \"$NORTH_WORKER_SESSION_SCOPE\" ] || printf '%s' \"$NORTH_WORKER_SESSION_SCOPE\" > {}\nprintf '%s|%s|%s\\n' \"$PWD\" \"$*\" \"$NORTH_FLEET_SESSION\" >> {}\n[ \"$1\" != safe-push ] || {{ sleep 8 </dev/null >/dev/null 2>&1 & }}\nprintf 'out %s\\n' \"$GH_TOKEN\"\nprintf 'err %s\\n' \"$GH_TOKEN\" >&2\n", root.path().join("scope").display(), log.display()));
        let environment = vec![("HOME".to_owned(), home.display().to_string()), ("PATH".to_owned(), "/run/current-system/sw/bin:/usr/bin:/bin".to_owned()), ("GH_TOKEN".to_owned(), HELD.to_owned())];
        let cgroups = root.path().join("cgroup");
        Self { host: Host { fleet, code: vec![home.join("code")], home, environment, cgroups }, root, north, lane, other }
    }

    fn ran(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.path().join("ran")).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    fn ask(&self, arguments: &[&str], directory: &Path) -> Asked {
        self.ask_from(&cgroup(), arguments, directory)
    }

    fn ask_from(&self, cgroup: &str, arguments: &[&str], directory: &Path) -> Asked {
        self.ask_with(cgroup, arguments, directory, Vec::new())
    }

    fn ask_with(&self, cgroup: &str, arguments: &[&str], directory: &Path, environment: Vec<(String, String)>) -> Asked {
        let (client, server) = UnixStream::pair().unwrap();
        let (mut out, out_writer) = std::io::pipe().unwrap();
        let (mut err, err_writer) = std::io::pipe().unwrap();
        let stdin = File::open("/dev/null").unwrap();
        let logged = std::thread::scope(|scope| {
            let served = scope.spawn(|| broker::answer(&self.north, &server, Some(cgroup), &self.host));
            let request = Request { arguments: arguments.iter().map(|word| (*word).to_owned()).collect(), directory: directory.to_owned(), environment };
            let code = broker::send(&client, request, [stdin.as_fd(), out_writer.as_fd(), err_writer.as_fd()]).unwrap();
            (code, served.join().unwrap())
        });
        drop((out_writer, err_writer));
        let (mut stdout, mut stderr) = (String::new(), String::new());
        out.read_to_string(&mut stdout).unwrap();
        err.read_to_string(&mut stderr).unwrap();
        Asked { code: logged.0, logged: logged.1.err().unwrap_or_default(), stdout, stderr }
    }
}

struct Asked {
    code: u8,
    logged: String,
    stdout: String,
    stderr: String,
}

impl Asked {
    fn refused(&self, naming: &str) {
        assert_eq!(self.code, 126, "{}", self.stderr);
        assert!(self.stderr.contains("refused") && self.stderr.contains(naming), "the client is told the refusal naming {naming}: {}", self.stderr);
        assert!(self.logged.contains("refused") && self.logged.contains(naming), "the broker log names the refusal of {naming}: {}", self.logged);
    }
}

fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
}

fn cgroup() -> String {
    format!("0::/user.slice/user-1000.slice/user@1000.service/app.slice/{UNIT}\n")
}

const OUTSIDE_CGROUP: &str = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/north-worker-outside.service\n";

#[test]
fn a_lane_only_session_lands_its_own_lane_on_main_and_nothing_else() {
    let fixture = Fixture::new();
    let finished = std::sync::OnceLock::new();
    std::thread::scope(|scope| {
        scope.spawn(|| finished.set(fixture.ask(&["safe-push", "--to", "main"], &fixture.lane)).is_ok());
        assert!(north_base::platform::wait::until(std::time::Duration::from_secs(4), || finished.get().is_some()), "the landing's output stayed open while a helper it left running lived, so the caller never read its end");
    });
    let landed = finished.into_inner().unwrap();
    assert_eq!(landed.code, 0, "{}", landed.stderr);
    assert_eq!(landed.stdout, "out [token withheld]\n");
    assert_eq!(fixture.ran(), [format!("{}|safe-push --to main|{NAME}", fixture.lane.display())]);
    fixture.ask(&["safe-push", "--to", "dev"], &fixture.lane).refused("dev");
    fixture.ask(&["safe-push", "--to", "main"], &fixture.other).refused(&fixture.other.display().to_string());
    fixture.ask(&["safe-push", "--to", "main", "--repo", "tompassarelli/wisp"], &fixture.lane).refused("--repo");
    assert_eq!(fixture.ran().len(), 1, "no refused request ran");
}

#[test]
fn a_lane_only_session_comments_on_and_ticks_only_its_own_task_issue() {
    let fixture = Fixture::new();
    for (arguments, expected) in [
        (vec!["task", "comment", "--body", "landed"], format!("task comment {TASK} --body landed")),
        (vec!["task", "comment", "north#1056", "--body", "landed"], format!("task comment {TASK} --body landed")),
        (vec!["task", "check", "--done-when", "--box", "lands", "#1056"], format!("task check --done-when --box lands {TASK}")),
    ] {
        let asked = fixture.ask(&arguments, &fixture.lane);
        assert_eq!(asked.code, 0, "{}", asked.stderr);
        assert_eq!(fixture.ran().last().unwrap(), &format!("{}|{expected}|{NAME}", fixture.lane.display()));
    }
    for issue in ["north#1", "wisp#1056", "tompassarelli/south#1056"] {
        fixture.ask(&["task", "comment", issue, "--body", "x"], &fixture.lane).refused(issue);
        fixture.ask(&["task", "check", "--done-when", "--box", "lands", issue], &fixture.lane).refused(issue);
    }
    assert_eq!(fixture.ran().len(), 3, "no refused request ran");
}

#[test]
fn no_token_reaches_the_lane_only_process_after_it_lands_and_comments() {
    let fixture = Fixture::new();
    let socket = fixture.root.path().join("broker.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let client = Command::new(std::env::current_exe().unwrap())
        .args([CLIENT, "--exact", "--ignored", "--nocapture", "--test-threads=1"])
        .current_dir(&fixture.lane)
        .env(SOCKET_ENV, &socket)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    for _ in 0..2 {
        let (stream, _) = listener.accept().unwrap();
        broker::answer(&fixture.north, &stream, Some(&cgroup()), &fixture.host).unwrap();
    }
    let output = client.wait_with_output().unwrap();
    let seen = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "{seen}");
    assert_eq!(fixture.ran().len(), 2, "both verbs ran with the token: {seen}");
    assert!(seen.contains("out [token withheld]") && seen.contains("err [token withheld]"), "{seen}");
    assert!(!seen.contains(HELD), "{seen}");
}

#[test]
#[ignore = "the lane-only process of no_token_reaches_the_lane_only_process_after_it_lands_and_comments"]
fn lane_only_client() {
    let socket = PathBuf::from(std::env::var_os(SOCKET_ENV).expect("run by no_token_reaches_the_lane_only_process_after_it_lands_and_comments"));
    let descriptors = || std::fs::read_dir("/dev/fd").unwrap().flatten().map(|entry| (entry.file_name(), std::fs::read_link(entry.path()).ok())).collect::<std::collections::BTreeSet<_>>();
    let before = descriptors();
    for arguments in [["safe-push", "--to", "main"].as_slice(), ["task", "comment", "--body", "landed"].as_slice()] {
        let stream = UnixStream::connect(&socket).unwrap();
        let request = Request { arguments: arguments.iter().map(|word| (*word).to_owned()).collect(), directory: std::env::current_dir().unwrap(), environment: std::env::vars().collect() };
        let (stdin, stdout, stderr) = (std::io::stdin(), std::io::stdout(), std::io::stderr());
        assert_eq!(broker::send(&stream, request, [stdin.as_fd(), stdout.as_fd(), stderr.as_fd()]).unwrap(), 0);
    }
    assert!(!std::env::vars().any(|(_, value)| value.contains(HELD)), "the token is in the lane-only process's environment");
    let after = descriptors();
    assert_eq!(before, after, "the broker handed the lane-only process a descriptor");
    for (_, target) in after.into_iter().filter_map(|(fd, target)| Some((fd, target?))).filter(|(_, target)| target.is_file()) {
        let text = std::fs::read(&target).unwrap_or_default();
        assert!(!String::from_utf8_lossy(&text).contains(HELD), "{} holds the token", target.display());
    }
}

#[test]
fn a_lane_only_opencode_worker_reaches_the_brokers_granted_path_without_moving_its_cgroup() {
    let fixture = Fixture::new();
    let scope = fixture.host.cgroups.join(cgroup().trim_start_matches("0::/").trim_end());
    std::fs::create_dir_all(&scope).unwrap();
    std::fs::write(scope.join("cgroup.procs"), "").unwrap();
    let started = fixture.ask(&["--worker-sandbox", "--lane", "--", "/bin/sh", "-c", "safe-push --to main"], &fixture.lane);
    assert_eq!(started.code, 0, "{}", started.stderr);
    if let Ok(procs) = std::fs::read_to_string(fixture.root.path().join("scope")) {
        super::super::session_join::join_scope(Path::new(&procs)).unwrap();
    }
    let child = if std::fs::read_to_string(scope.join("cgroup.procs")).unwrap() == "0" { cgroup() } else { OUTSIDE_CGROUP.to_owned() };
    let landed = fixture.ask_from(&child, &["safe-push", "--to", "main"], &fixture.lane);
    assert_eq!(landed.code, 0, "the brokered child's nested safe-push is granted: {}", landed.stderr);
    assert_eq!(fixture.ran().last().unwrap(), &format!("{}|safe-push --to main|{NAME}", fixture.lane.display()));
    let ticked = fixture.ask_from(&child, &["task", "check", "--done-when", "--box", "a box"], &fixture.lane);
    assert_eq!(ticked.code, 0, "the brokered child's nested task check is granted: {}", ticked.stderr);
}

#[test]
fn a_claude_workers_safe_push_runs_outside_with_its_own_environment_and_reads_only_what_it_could_north_1094() {
    let fixture = Fixture::new();
    let worker = "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-fleet-n1094-x.service\n";
    let held = fixture.host.home.join(north_base::platform::session_host_files::HELD).join("north-fleet-n1094-x.service");
    let placed = fixture.host.fleet.join(north_base::platform::session_host_files::PLACED).join("c0ffee-n1094");
    std::fs::create_dir_all(&held).unwrap();
    std::fs::create_dir_all(&placed).unwrap();
    std::fs::write(held.join(north_base::platform::session_host_files::BOUND_CONVERSATION), "c0ffee-n1094").unwrap();
    std::fs::write(crate::fleet::records::record_file(&placed), serde_json::json!({ "name": "n1094-x", "provider": "claude" }).to_string()).unwrap();
    let own = vec![("HOME".to_owned(), fixture.host.home.display().to_string()), ("NORTH_FLEET_SESSION".to_owned(), "n1094-x".to_owned()), ("NORTH_WORKER_SANDBOXED".to_owned(), "1".to_owned())];
    let landed = fixture.ask_with(worker, &["safe-push", "--to", "main"], &fixture.lane, own.clone());
    assert_eq!(landed.code, 0, "{}", landed.stderr);
    assert_eq!(landed.stdout, "out \n", "the host's token does not reach a worker that holds its own");
    assert_eq!(fixture.ran(), [format!("{}|safe-push --to main|n1094-x", fixture.lane.display())]);
    let login = fixture.host.home.join(".config/gh/hosts.yml");
    std::fs::create_dir_all(login.parent().unwrap()).unwrap();
    std::fs::write(&login, "oauth_token: x\n").unwrap();
    let leaked = fixture.ask_with(worker, &["safe-push", "--to", "main", "--message", login.to_str().unwrap()], &fixture.lane, own);
    assert_eq!(leaked.code, 126);
    assert!(leaked.logged.contains("outside the files the caller can read"), "{}", leaked.logged);
    assert_eq!(fixture.ran().len(), 1, "a message file the worker cannot read is never handed to safe-push");
}

#[test]
fn a_lane_whose_git_config_sets_a_filter_driver_or_ssh_command_is_not_landed_and_its_filter_does_not_run() {
    let fixture = Fixture::new();
    let git = |arguments: &[&str]| assert!(Command::new("git").arg("-C").arg(&fixture.lane).args(arguments).status().unwrap().success());
    git(&["init", "-q"]);
    let ran = fixture.root.path().join("filter-ran");
    git(&["config", "filter.evil.clean", &format!("touch {}", ran.display())]);
    fixture.ask(&["safe-push", "--to", "main"], &fixture.lane).refused("filter.evil.clean");
    git(&["config", "--unset", "filter.evil.clean"]);
    git(&["config", "core.sshCommand", "touch pwned"]);
    fixture.ask(&["safe-push", "--to", "main"], &fixture.lane).refused("core.sshcommand");
    assert!(!ran.exists() && fixture.ran().is_empty(), "nothing ran for a refused lane");
    git(&["config", "--unset", "core.sshCommand"]);
    assert_eq!(fixture.ask(&["safe-push", "--to", "main"], &fixture.lane).code, 0);
}
