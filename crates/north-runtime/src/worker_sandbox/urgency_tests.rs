use std::fs::File;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::foundation::offload::URGENT_ENV;
use north_base::foundation::shim::write_executable;
use north_base::platform::session_host_files;

use super::super::broker::{self, Request};
use super::super::lane_grant::Host;
use crate::capacity::host::urgency::Places;
use crate::land_offload::ship::SlotClass;
use crate::land_offload::ship::tests::{Queued, order_through_one_slot};

const URGENT_SCOPE: &str = "north-session-0f0f0f0f-urgent.scope";
const NORMAL_SCOPE: &str = "north-session-0e0e0e0e-normal.scope";
const HELD_UNIT: &str = "north-fleet-held.service";
const PLACED: &str = "c0ffee00-placed";
const BROKER: &str = "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-worker-outside.service\n";
const SHELL: &str = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/foot-server.scope\n";

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
        let lane = home.join("code/north/worktrees/urgent");
        let fleet = root.path().join("fleet");
        let bin = root.path().join("bin");
        for directory in [&main, &bin, &fleet.join(session_host_files::PLACED).join(PLACED), &home.join(session_host_files::HELD).join(HELD_UNIT)] {
            std::fs::create_dir_all(directory).unwrap();
        }
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "one"]);
        git(&main, &["worktree", "add", "-q", lane.to_str().unwrap(), "-b", "urgent"]);
        for (name, unit, urgent) in [("coordinated", URGENT_SCOPE, true), ("worker", NORMAL_SCOPE, false)] {
            std::fs::create_dir_all(fleet.join(name)).unwrap();
            std::fs::write(session_host_files::scope_path(&fleet.join(name)), format!("{unit}\n")).unwrap();
            std::fs::write(crate::fleet::records::record_file(&fleet.join(name)), serde_json::json!({ "name": name, "urgent": urgent }).to_string()).unwrap();
        }
        std::fs::write(home.join(session_host_files::HELD).join(HELD_UNIT).join(session_host_files::BOUND_CONVERSATION), PLACED).unwrap();
        std::fs::write(crate::fleet::records::record_file(&fleet.join(session_host_files::PLACED).join(PLACED)), serde_json::json!({ "name": "placed", "urgent": true }).to_string()).unwrap();
        for program in ["ssh", "git"] {
            write_executable(&bin.join(program), "#!/bin/sh\nexit 0\n");
        }
        let north = root.path().join("north");
        write_executable(&north, format!("#!/bin/sh\nprintf '%s' \"${{{URGENT_ENV}:-}}\" > {}\n", root.path().join("mark").display()));
        let environment = vec![("HOME".to_owned(), home.display().to_string()), ("PATH".to_owned(), bin.display().to_string())];
        let host = Host { fleet, code: vec![home.join("code")], home, environment, cgroups: root.path().join("cgroup") };
        Self { root, host, north, lane }
    }

    fn mark(&self, cgroup: &str, words: &str) -> String {
        let _ = std::fs::remove_file(self.root.path().join("mark"));
        let (client, server) = UnixStream::pair().unwrap();
        let (_out, out_writer) = std::io::pipe().unwrap();
        let (_err, err_writer) = std::io::pipe().unwrap();
        let stdin = File::open("/dev/null").unwrap();
        let claimed = vec![(URGENT_ENV.to_owned(), "1".to_owned())];
        std::thread::scope(|scope| {
            let served = scope.spawn(|| broker::answer(&self.north, &server, Some(cgroup), &self.host));
            let request = Request { arguments: words.split(' ').map(str::to_owned).collect(), directory: self.lane.clone(), environment: claimed };
            broker::send(&client, request, [stdin.as_fd(), out_writer.as_fd(), err_writer.as_fd()]).unwrap();
            served.join().unwrap().unwrap_or_else(|why| panic!("{words} from {cgroup}: {why}"));
        });
        std::fs::read_to_string(self.root.path().join("mark")).unwrap_or_else(|_| format!("{words} started no north"))
    }

    fn places(&self) -> Places {
        Places { fleet: self.host.fleet.clone(), home: self.host.home.clone(), procs: self.root.path().join("proc"), cgroups: self.root.path().join("cgroup") }
    }

    fn process(&self, pid: u32, parent: u32, cgroup: &str, mark: &str) -> String {
        let directory = self.root.path().join("proc").join(pid.to_string());
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("cgroup"), cgroup).unwrap();
        std::fs::write(directory.join("stat"), format!("{pid} (a (b) c) S {parent} {pid} {pid} 0\n")).unwrap();
        std::fs::write(directory.join("environ"), format!("HOME=/h\0{URGENT_ENV}={mark}\0")).unwrap();
        pid.to_string()
    }

    fn lease(&self, number: u32, members: &[u32]) -> String {
        let path = format!("user.slice/user-1000.slice/user@1000.service/agent.slice/agent-capacity.slice/agent-capacity-{number:032x}.scope");
        std::fs::create_dir_all(self.root.path().join("cgroup").join(&path)).unwrap();
        std::fs::write(self.root.path().join("cgroup").join(&path).join("cgroup.procs"), members.iter().map(|pid| format!("{pid}\n")).collect::<String>()).unwrap();
        format!("0::/{path}\n")
    }
}

#[test]
fn an_urgent_job_queued_after_a_normal_one_is_taken_first() {
    let fixture = Fixture::new();
    let gate = "safe-push --to main";
    let offload = "capacity offload slots";
    let mut failures = Vec::new();
    for (caller, words, expected) in [(in_unit(URGENT_SCOPE), gate, "1"), (in_unit(URGENT_SCOPE), offload, "1"), (in_unit(HELD_UNIT), gate, "1"), (in_unit(NORMAL_SCOPE), gate, ""), (in_unit(NORMAL_SCOPE), offload, "")] {
        let seen = fixture.mark(&caller, words);
        if seen != expected {
            failures.push(format!("the broker gave `{words}` from {} the urgent mark {seen:?}, not {expected:?}", caller.trim()));
        }
    }
    let places = fixture.places();
    let word = |urgent: bool| if urgent { "1" } else { "" };
    fixture.process(1, 0, "0::/init.scope\n", "");
    fixture.process(10, 1, &in_unit(URGENT_SCOPE), "");
    fixture.process(11, 1, &in_unit(NORMAL_SCOPE), "1");
    fixture.process(20, 1, BROKER, "");
    fixture.process(21, 20, BROKER, &fixture.mark(&in_unit(URGENT_SCOPE), gate));
    fixture.process(22, 20, BROKER, &fixture.mark(&in_unit(NORMAL_SCOPE), gate));
    let (lease_a, lease_b, lease_c, lease_d, lease_e) = (fixture.lease(0xa, &[31]), fixture.lease(0xb, &[32]), fixture.lease(0xc, &[33]), fixture.lease(0xd, &[34]), fixture.lease(0xe, &[35]));
    fixture.process(31, 21, &lease_a, "");
    let normal_gate = word(places.process(&fixture.process(32, 22, &lease_b, "1")));
    let urgent_lease = word(places.process(&fixture.process(33, 10, &lease_c, "")));
    let normal_lease = word(places.process(&fixture.process(34, 11, &lease_d, "1")));
    let urgent_gate = word(places.cgroup(&lease_a));
    std::fs::create_dir_all(fixture.root.path().join("leases")).unwrap();
    std::fs::write(fixture.root.path().join("leases/0000000e-0000-0000-0000-000000000000.json"), serde_json::json!({ "wrapperPid": 10 }).to_string()).unwrap();
    let posers = [
        ("a job in a normal session's lease whose lease record names an urgent session's process as its wrapper", fixture.process(35, 11, &lease_e, "1")),
        ("a job from a shell outside every session, which set the urgent mark itself", fixture.process(36, 1, SHELL, "1")),
        ("a job in a broker-named unit nested in a normal session's scope, which set the urgent mark itself", fixture.process(37, 11, &in_unit(&format!("{NORMAL_SCOPE}/north-worker-outside.service")), "1")),
        ("a job in a unit named only like the broker, which set the urgent mark itself", fixture.process(38, 1, &in_unit("north-worker-outside-evil.service"), "1")),
    ];
    for (poser, pid) in posers {
        if places.process(&pid) {
            failures.push(format!("{poser} was judged urgent"));
        }
    }
    let job = |label, hold, urgent| Queued { label, name: "code-p-main", carried: "", red_fix: "", owner: "", urgent, hold, class: SlotClass::LandCheck };
    let order = order_through_one_slot(&[job("holder", true, ""), job("normal-gate", false, normal_gate), job("normal-lease", false, normal_lease), job("urgent-gate", false, urgent_gate), job("urgent-lease", false, urgent_lease)]);
    let expected = ["start-holder", "end-holder", "start-urgent-gate", "end-urgent-gate", "start-urgent-lease", "end-urgent-lease", "start-normal-gate", "end-normal-gate", "start-normal-lease", "end-normal-lease"];
    if order != expected {
        failures.push(format!("the urgent session's brokered gate job and leased offload, queued behind the normal session's, which each claimed the urgent mark, did not take the freed slot first: {order:?}"));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
