use std::fs::File;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::foundation::shim::write_executable;
use north_base::platform::session_host_files;

use super::super::broker::{self, Request};
use super::super::lane_grant::Host;
use super::super::peer::Peer;

const WORKER_SCOPE: &str = "north-session-0a0a0a0a-worker.scope";
const PROXY_SCOPE: &str = "north-session-0b0b0b0b-proxy.scope";
const UNRECORDED_SCOPE: &str = "north-session-0c0c0c0c-unrecorded.scope";
const UNNAMED_SCOPE: &str = "north-session-0d0d0d0d-unnamed.scope";

fn in_unit(unit: &str) -> String {
    format!("0::/user.slice/user-1000.slice/user@1000.service/agent.slice/{unit}\n")
}

fn git(directory: &Path, arguments: &[&str]) {
    assert!(Command::new("git").args(arguments).current_dir(directory).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").status().unwrap().success());
}

struct Fixture {
    root: tempfile::TempDir,
    host: Host,
    north: PathBuf,
    lane: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let main = home.join("code/north/main");
        let lane = home.join("code/north/worktrees/forged");
        let fleet = root.path().join("fleet");
        let bin = root.path().join("bin");
        for directory in [&main, &bin, &fleet] {
            std::fs::create_dir_all(directory).unwrap();
        }
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "one"]);
        git(&main, &["worktree", "add", "-q", lane.to_str().unwrap(), "-b", "forged"]);
        for (name, unit, record) in [("worker", WORKER_SCOPE, Some(serde_json::json!({ "name": "worker", "role": "worker" }))), ("proxy", PROXY_SCOPE, Some(serde_json::json!({ "name": "proxy", "role": "proxy" }))), ("unnamed", UNNAMED_SCOPE, Some(serde_json::json!({ "name": "look-lead" }))), ("unrecorded", UNRECORDED_SCOPE, None)] {
            std::fs::create_dir_all(fleet.join(name)).unwrap();
            std::fs::write(session_host_files::scope_path(&fleet.join(name)), format!("{unit}\n")).unwrap();
            if let Some(record) = record {
                std::fs::write(crate::fleet::records::record_file(&fleet.join(name)), record.to_string()).unwrap();
            }
        }
        for program in ["ssh", "git"] {
            write_executable(&bin.join(program), "#!/bin/sh\nexit 0\n");
        }
        let north = root.path().join("north");
        write_executable(&north, format!("#!/bin/sh\nprintf '%s' \"$*\" > {}\n", root.path().join("started").display()));
        let environment = vec![("HOME".to_owned(), home.display().to_string()), ("PATH".to_owned(), bin.display().to_string())];
        let host = Host { fleet, code: vec![home.join("code")], home, environment, cgroups: root.path().join("cgroup") };
        Self { root, host, north, lane }
    }

    fn started(&self, cgroup: &str, words: &str, forged: &str) -> String {
        let _ = std::fs::remove_file(self.root.path().join("started"));
        let (client, server) = UnixStream::pair().unwrap();
        let (_out, out_writer) = std::io::pipe().unwrap();
        let (_err, err_writer) = std::io::pipe().unwrap();
        let stdin = File::open("/dev/null").unwrap();
        let environment = vec![("NORTH_FLEET_ROLE".to_owned(), forged.to_owned()), ("NORTH_WORKER_SANDBOX".to_owned(), "0".to_owned())];
        std::thread::scope(|scope| {
            let served = scope.spawn(|| broker::answer(&self.north, &server, &Peer::from(Some(cgroup)), &self.host));
            let request = Request { arguments: words.split(' ').map(str::to_owned).collect(), directory: self.lane.clone(), environment };
            broker::send(&client, request, [stdin.as_fd(), out_writer.as_fd(), err_writer.as_fd()]).unwrap();
            served.join().unwrap().unwrap_or_else(|why| panic!("{words} from {cgroup}: {why}"));
        });
        std::fs::read_to_string(self.root.path().join("started")).unwrap_or_else(|_| format!("{words} started no north"))
    }
}

#[test]
fn capability_record_confines_a_worker_whatever_role_its_environment_forges() {
    let fixture = Fixture::new();
    let north = fixture.north.display().to_string();
    let built = "--worker-sandbox --built -- true";
    let run = "capacity host run --class heavy --owner o -- true";
    let cases = [
        ("a worker that forged the proxy role", WORKER_SCOPE, "proxy", built, "--worker-sandbox --built --worker -- true".to_owned()),
        ("a worker that forged the proxy role", WORKER_SCOPE, "proxy", run, format!("capacity host run --class heavy --owner o -- {north} --worker-sandbox --program --worker -- true")),
        ("a session whose record cannot be read", UNRECORDED_SCOPE, "proxy", built, "--worker-sandbox --built --worker -- true".to_owned()),
        ("a lead whose record names no role", UNNAMED_SCOPE, "proxy", built, "--worker-sandbox --built --worker -- true".to_owned()),
        ("the proxy seat that forged a worker's role", PROXY_SCOPE, "worker", built, "--worker-sandbox --built -- true".to_owned()),
    ];
    let mut failures = Vec::new();
    for (who, scope, forged, words, expected) in cases {
        let started = fixture.started(&in_unit(scope), words, forged);
        if started != expected {
            failures.push(format!("the broker started `{started}` for `{words}` from {who}, not `{expected}`, so a domain followed the role the client sent (north#1574)"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn capability_record_grants_no_eval_lane_for_a_test_root_the_worker_forges() {
    let home = Path::new("/home/t");
    let work = home.join("eval-data/eval-a/work/task-1");
    let lane = work.join("p/worktrees/worker");
    let sent = [("HOME".to_owned(), home.display().to_string()), ("NORTH_TEST_ROOT".to_owned(), work.join("state").display().to_string())];
    let own = [("HOME".to_owned(), home.display().to_string())];
    let forged = super::super::Context::from_caller("", &lane, &sent, &own);
    assert!(forged.named_lanes.is_empty(), "the broker granted {:?} from a NORTH_TEST_ROOT the client sent (north#1574)", forged.named_lanes);
}
