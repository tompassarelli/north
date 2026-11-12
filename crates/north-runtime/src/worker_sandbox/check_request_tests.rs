use std::fs::File;
use std::io::Read;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::foundation::shim::write_executable;
use north_base::platform::gh_shim;

use super::super::broker::{self, Request};
use super::Host;

const NAME: &str = "w-built-verify";
const CGROUP: &str = "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-session-398cca43.scope\n";
const BOX: &str = "The runtime watch counts no session stopped on purpose";
const ISSUE: &str = "tompassarelli/north#1022";

struct Fixture {
    _root: tempfile::TempDir,
    host: Host,
    north: PathBuf,
    main: PathBuf,
    lane: PathBuf,
    top: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let top = root.path().canonicalize().unwrap();
        let home = top.join("home");
        let main = home.join("code/north/main");
        let lane = home.join("code/north/worktrees").join(NAME);
        let shim = home.join(".local/bin");
        let fleet = top.join("fleet");
        let programs = top.join("bin");
        for directory in [&main, &lane, &shim, &programs, &fleet.join(NAME)] {
            std::fs::create_dir_all(directory).unwrap();
        }
        write_executable(&programs.join("ssh"), "#!/bin/sh\n");
        assert!(Command::new("sh").args(["-c", "git init -q && git remote add origin https://github.com/tompassarelli/north.git"]).current_dir(&main).status().unwrap().success());
        write_executable(&shim.join("gh"), format!("#!/bin/sh\n# {}\necho 'echo the command the issue holds'\n", gh_shim::MARKER));
        write_executable(&lane.join("gh"), "#!/bin/sh\necho 'echo the command the lane holds'\n");
        std::fs::write(fleet.join(NAME).join("host.scope"), "north-session-398cca43.scope\n").unwrap();
        std::fs::write(fleet.join(NAME).join("session.json"), serde_json::json!({ "name": NAME, "cwd": lane, "task_ref": ISSUE }).to_string()).unwrap();
        let north = top.join("north");
        write_executable(&north, "#!/bin/sh\nprintf '%s|%s|%s|%s\\n' \"$PWD\" \"$*\" \"$NORTH_FLEET_SESSION\" \"$FROM_THE_LANE\"\nsh -c \"$(gh)\"\nif [ -p \"$0.started\" ] && mkdir \"$0.first\" 2>/dev/null; then : > \"$0.started\"; cat \"$0.release\" > /dev/null; fi\n");
        let environment = vec![("HOME".to_owned(), home.display().to_string()), ("PATH".to_owned(), format!("{}:{}", programs.display(), std::env::var("PATH").unwrap()))];
        Self { host: Host { fleet, code: vec![home.join("code")], home, environment, cgroups: top.join("cgroup") }, _root: root, north, main, lane, top }
    }

    fn ask(&self, cgroup: &str, arguments: &[&str], directory: &Path) -> (u8, String, String) {
        let (client, server) = UnixStream::pair().unwrap();
        let (mut out, out_writer) = std::io::pipe().unwrap();
        let (mut err, err_writer) = std::io::pipe().unwrap();
        let stdin = File::open("/dev/null").unwrap();
        let environment = vec![("PATH".to_owned(), format!("{}:{}", self.lane.display(), std::env::var("PATH").unwrap())), ("FROM_THE_LANE".to_owned(), "1".to_owned())];
        let code = std::thread::scope(|scope| {
            scope.spawn(|| broker::answer(&self.north, &server, Some(cgroup), &self.host));
            let request = Request { arguments: arguments.iter().map(|word| (*word).to_owned()).collect(), directory: directory.to_owned(), environment };
            broker::send(&client, request, [stdin.as_fd(), out_writer.as_fd(), err_writer.as_fd()]).unwrap()
        });
        drop((out_writer, err_writer));
        let (mut stdout, mut stderr) = (String::new(), String::new());
        out.read_to_string(&mut stdout).unwrap();
        err.read_to_string(&mut stderr).unwrap();
        (code, stdout, stderr)
    }
}

#[test]
fn a_check_asked_from_a_lane_that_holds_another_command_for_the_box_runs_the_issues_command_and_takes_nothing_of_the_lane() {
    let fixture = Fixture::new();
    let (code, stdout, stderr) = fixture.ask(CGROUP, &["task", "check", "--done-when", "--box", BOX, ISSUE], &fixture.lane);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, format!("{}|task check --done-when --confined --box {BOX} {ISSUE}|{NAME}|\nthe command the issue holds\n", fixture.main.display()), "the checker starts in the registered main checkout with the broker's PATH and none of the caller's variables");
    for (arguments, naming) in [
        (vec!["task", "check", "--done-when", "--box", BOX, "--main", fixture.lane.to_str().unwrap(), ISSUE], "takes no other word"),
        (vec!["task", "check", "--done-when", "--body-file", "boxes.md"], "takes no other word"),
        (vec!["task", "check", "--done-when", ISSUE], "takes no other word"),
        (vec!["task", "check", "--done-when", "--box", BOX, "north#1022"], "OWNER/REPO#N"),
        (vec!["task", "check", "--done-when", "--box", BOX, "tompassarelli/wisp#3"], "its own task issue"),
        (vec!["task", "check", "--done-when", "--box", BOX, "someone/north#1022"], "its own task issue"),
        (vec!["task", "check", "--done-when", "--box", BOX, "tompassarelli/north#1048"], "its own task issue"),
    ] {
        let (code, stdout, stderr) = fixture.ask(CGROUP, &arguments, &fixture.lane);
        assert_eq!((code, stdout.as_str()), (126, ""), "{arguments:?} started nothing: {stderr}");
        assert!(stderr.contains("refused") && stderr.contains(naming), "{arguments:?} is refused naming {naming}: {stderr}");
    }
    let outside = CGROUP.replace("north-session-398cca43.scope", "north-worker-outside.service");
    let (code, stdout, stderr) = fixture.ask(&outside, &["task", "check", "--done-when", "--box", BOX, ISSUE], &fixture.lane);
    assert_eq!((code, stdout.as_str()), (126, ""), "{stderr}");
    assert!(stderr.contains("fleet session's own scope"), "{stderr}");
    let (started, release) = (fixture.top.join("north.started"), fixture.top.join("north.release"));
    assert!(Command::new("mkfifo").args([&started, &release]).status().unwrap().success());
    let words = ["task", "check", "--done-when", "--box", BOX, ISSUE];
    std::thread::scope(|scope| {
        let first = scope.spawn(|| fixture.ask(CGROUP, &words, &fixture.lane));
        std::fs::read(&started).unwrap();
        let (code, stdout, stderr) = fixture.ask(CGROUP, &words, &fixture.lane);
        std::fs::remove_file(&started).unwrap();
        std::fs::write(&release, "").unwrap();
        assert_eq!((code, stdout.as_str()), (126, ""), "a second check asked while the session's first still ran started a checker: {stderr}");
        assert!(stderr.contains("one at a time"), "{stderr}");
        let (code, _, stderr) = first.join().unwrap();
        assert_eq!(code, 0, "{stderr}");
    });
    let (code, _, stderr) = fixture.ask(CGROUP, &words, &fixture.lane);
    assert_eq!(code, 0, "a check asked after the session's first ended is run: {stderr}");
}
