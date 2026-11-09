use std::path::{Path, PathBuf};
use std::process::Command;

use super::lane::{Lane, gather};
use super::room_for;
use super::remote::{Ran, drive, hand_over, invocation};
use super::take::{Sender, Starting, Target, hang_up, pick, refusal, release_command, run_lane, transport};
use crate::capacity::build_offload::{Admission, Declined, Job};
use crate::capacity::handover::Offer;
use crate::dispatch::measure::Measure;
use crate::land_offload::room::Room;

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

struct Hosts {
    _scratch: tempfile::TempDir,
    laptop: PathBuf,
    remote: PathBuf,
}

impl Hosts {
    fn main(&self) -> PathBuf {
        self.laptop.join("code/proj/main")
    }

    fn lane(&self, name: &str) -> PathBuf {
        let lane = self.laptop.join("code/proj/worktrees").join(name);
        git(&self.main(), &["worktree", "add", "-q", "-b", name, lane.to_str().unwrap(), "origin/main"]);
        lane
    }
}

fn hosts() -> Hosts {
    let scratch = tempfile::tempdir().unwrap();
    let (laptop, remote, origin) = (scratch.path().join("laptop"), scratch.path().join("gex45"), scratch.path().join("origin.git"));
    git(scratch.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    let main = laptop.join("code/proj/main");
    git(scratch.path(), &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
    git(&main, &["checkout", "-q", "-B", "main"]);
    write(&main.join("a.txt"), "main\n");
    git(&main, &["add", "a.txt"]);
    git(&main, &["commit", "-q", "-m", "seed"]);
    write(&main.join("c.txt"), "second\n");
    git(&main, &["add", "c.txt"]);
    git(&main, &["commit", "-q", "-m", "second"]);
    git(&main, &["push", "-q", "origin", "main"]);
    std::fs::create_dir_all(remote.join("code/proj")).unwrap();
    git(scratch.path(), &["clone", "-q", origin.to_str().unwrap(), remote.join("code/proj/main").to_str().unwrap()]);
    Hosts { _scratch: scratch, laptop, remote }
}

const REPORT: &str = "printf 'head %s\\n' \"$(git rev-parse HEAD)\"; printf 'where %s\\n' \"$(pwd)\"; git status --porcelain | sed 's/^/status /'; for f in a.txt b.txt; do [ -e $f ] && printf 'file %s %s\\n' $f \"$(cat $f)\"; done; true";

fn ran_remotely(hosts: &Hosts, lane: &Lane) -> Vec<String> {
    ran_remotely_with(hosts, lane, REPORT)
}

fn ran_remotely_with(hosts: &Hosts, lane: &Lane, report: &str) -> Vec<String> {
    let command = ["sh".to_owned(), "-c".to_owned(), report.to_owned()];
    ran_command(hosts, lane, "while [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n", &command)
}

fn ran_command(hosts: &Hosts, lane: &Lane, stub_body: &str, command: &[String]) -> Vec<String> {
    let stub = hosts.remote.join("north-stub");
    crate::foundation::shim::write_script(&stub, stub_body);
    let returned = "NORTH-OFFLOAD-RETURNED-test";
    let (script, words) = invocation(lane, &Job { owner: "t", timeout_seconds: 60, command, cpus: 6.0 }, returned);
    let mut transport = Command::new("sh");
    transport.args(["-c", &script, "offload"]).args(&words).env("HOME", &hosts.remote).env("AGENT_CAPACITY_NORTH", &stub);
    let output = hosts.remote.join("output.log");
    let ran = drive(transport, lane, Some(std::fs::File::create(&output).unwrap()), &|| true, returned);
    assert!(matches!(ran, Ran::Exit(0)));
    std::fs::read_to_string(&output).unwrap().lines().map(str::to_owned).collect()
}

fn line<'a>(report: &'a [String], key: &str) -> Vec<&'a str> {
    report.iter().filter_map(|line| line.strip_prefix(key)).map(str::trim_start).collect()
}

#[test]
fn capacity_an_offloaded_lane_with_unpushed_commits_and_uncommitted_changes_runs_against_exactly_the_lane_tree_never_the_hosts_main() {
    let hosts = hosts();
    let lane_dir = hosts.lane("flow-alerts");
    write(&lane_dir.join("a.txt"), "committed lane\n");
    write(&lane_dir.join("Cargo.lock"), "resolved before\n");
    write(&lane_dir.join("crates/x/Cargo.lock"), "nested before\n");
    git(&lane_dir, &["add", "Cargo.lock", "crates/x/Cargo.lock"]);
    git(&lane_dir, &["commit", "-q", "-am", "unpushed"]);
    write(&lane_dir.join("a.txt"), "dirty lane\n");
    write(&lane_dir.join("b.txt"), "untracked\n");
    let head = git(&lane_dir, &["rev-parse", "HEAD"]);
    assert_ne!(head, git(&lane_dir, &["rev-parse", "origin/main"]));
    let lane = gather(&lane_dir, &hosts.laptop).unwrap().ship(&[]).unwrap();
    let report = ran_remotely_with(&hosts, &lane, &format!("{REPORT}; printf 'resolved after\\n' >Cargo.lock"));
    assert_eq!(line(&report, "head"), [head.as_str()]);
    assert_eq!(std::fs::read_to_string(lane_dir.join("Cargo.lock")).unwrap(), "resolved after\n", "a lock the run changed comes back to the lane (north#671)");
    assert_eq!(std::fs::read_to_string(lane_dir.join("crates/x/Cargo.lock")).unwrap(), "nested before\n");
    let main = hosts.remote.join("code/proj/main");
    assert_ne!(line(&report, "where"), [main.to_str().unwrap()]);
    assert_eq!(line(&report, "where"), [hosts.remote.join(".cache/build-offload/code-proj-main-flow-alerts").to_str().unwrap()]);
    assert_eq!(line(&report, "status"), ["M a.txt", "?? b.txt"]);
    assert_eq!(line(&report, "file"), ["a.txt dirty lane", "b.txt untracked"]);
    assert_ne!(git(&main, &["rev-parse", "HEAD"]), head);
}


#[test]
fn capacity_an_offloaded_post_land_worktree_outside_the_project_container_rebuilds_from_the_repositorys_main_checkout() {
    let hosts = hosts();
    let lane_dir = hosts.laptop.join("code/proj-data/land-queue/key/post-land-abc");
    git(&hosts.main(), &["worktree", "add", "-q", "--detach", lane_dir.to_str().unwrap(), "origin/main"]);
    write(&lane_dir.join("a.txt"), "landed\n");
    git(&lane_dir, &["commit", "-q", "-am", "landed"]);
    let head = git(&lane_dir, &["rev-parse", "HEAD"]);
    let lane = gather(&lane_dir, &hosts.laptop).unwrap().ship(&[]).unwrap();
    assert_eq!(lane.repo, "code/proj/main");
    let report = ran_remotely(&hosts, &lane);
    assert_eq!(line(&report, "head"), [head.as_str()]);
    assert_eq!(line(&report, "file"), ["a.txt landed"]);
}

#[test]
fn capacity_an_offloaded_command_naming_a_runtime_the_host_lacks_runs_the_hosts_own_north() {
    let hosts = hosts();
    let lane = gather(&hosts.lane("host-north"), &hosts.laptop).unwrap().ship(&[]).unwrap();
    let stub = "printf 'north %s\\n' \"$*\"\n";
    let laptop_only = "/laptop-only/north-data/runtimes/f2ae888b5c28/north".to_owned();
    let command = [laptop_only, "land".to_owned(), "checks".to_owned(), "--leased".to_owned()];
    let report = ran_command(&hosts, &lane, stub, &command);
    assert_eq!(line(&report, "north"), ["capacity host run --class heavy --owner t --timeout-seconds 60 -- ".to_owned() + hosts.remote.join("north-stub").to_str().unwrap() + " land checks --leased"]);
    let present = hosts.remote.join("runtimes/north");
    std::fs::create_dir_all(present.parent().unwrap()).unwrap();
    crate::foundation::shim::write_script(&present, "true\n");
    let command = [present.to_str().unwrap().to_owned(), "land".to_owned()];
    let report = ran_command(&hosts, &lane, stub, &command);
    assert!(line(&report, "north")[0].ends_with(&format!("-- {} land", present.display())), "a runtime path that exists on the host stays as given: {report:?}");
    let report = ran_command(&hosts, &lane, stub, &["north".to_owned(), "depbox".to_owned()]);
    assert!(line(&report, "north")[0].ends_with(&format!("-- {} depbox", hosts.remote.join("north-stub").display())), "the bare north the depbox wrapper names is the host's own: {report:?}");
}

fn laptop_room(committed: f64) -> Room {
    let admission = Admission { limit_cpus: 14.0, committed_cpus: committed, queued_heavy_cpus: 0.0, cpu_pressure_percent: 1.0, pressure_high_percent: 25.0, land_check_waiting: false, heavy_settling_seconds: 0.0, host_cpu_pressure_percent: 0.0, job_peak_mib: 8192.0 };
    Room { measure: Measure { cores: 20.0, memory_total_mib: 64_000.0, memory_available_mib: 50_000.0, load: 1.0 }, admission, disk_free_gib: 200.0, slots: None }
}

#[test]
fn capacity_a_queued_run_handed_to_a_taker_runs_on_the_taker_and_its_output_and_exit_come_back() {
    let hosts = hosts();
    let lane_dir = hosts.lane("pulled");
    write(&lane_dir.join("b.txt"), "untracked\n");
    let lane = gather(&lane_dir, &hosts.laptop).unwrap().ship(&[]).unwrap();
    let stub = hosts.remote.join("north-stub");
    crate::foundation::shim::write_script(&stub, "while [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n");
    let command = ["sh".to_owned(), "-c".to_owned(), "printf 'where %s\\n' \"$(pwd)\"; cat b.txt; printf taken >Cargo.lock; echo problem >&2; exit 7".to_owned()];
    let (queued, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let remote = hosts.remote.clone();
    let taker = std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(peer.try_clone().unwrap());
        let mut first = String::new();
        std::io::BufRead::read_line(&mut reader, &mut first).unwrap();
        let offer = Offer::from_line(&first).expect("the queue's host sends one offer line");
        let sender = std::sync::Arc::new(Sender::to(peer));
        let starting = std::sync::Arc::new(Starting { count: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(1)), released: std::sync::atomic::AtomicBool::new(false) });
        let environment = |command: &mut Command| {
            command.env("HOME", &remote).env("AGENT_CAPACITY_NORTH", remote.join("north-stub"));
        };
        let (status, synced) = run_lane(&offer, "ccx33", None, reader, &sender, &starting, &environment).expect("the script starts");
        assert!(synced);
        sender.send(&crate::capacity::handover::Frame::Exit(status));
    });
    let sink = hosts.remote.join("out.txt");
    let announced = std::sync::Mutex::new(Vec::new());
    let job = Job { owner: "t", timeout_seconds: 60, command: &command, cpus: 6.0 };
    let code = hand_over(queued, &lane, &job, Some(std::fs::File::create(&sink).unwrap()), &|host| {
        announced.lock().unwrap().push(host.to_owned());
        true
    }).expect("the taker ran it");
    taker.join().unwrap();
    assert_eq!(code, 7);
    assert_eq!(*announced.lock().unwrap(), ["ccx33"]);
    let output = std::fs::read_to_string(&sink).unwrap();
    assert!(output.contains(&format!("where {}", hosts.remote.join(".cache/build-offload/code-proj-main-pulled").display())), "{output}");
    assert!(output.contains("untracked"), "{output}");
    assert_eq!(std::fs::read_to_string(lane_dir.join("Cargo.lock")).unwrap(), "taken", "a lock a handed-over run changed comes back to the lane (north#671)");
}

#[test]
fn capacity_a_run_stays_queued_when_the_taker_has_no_room_cannot_rebuild_the_lane_or_leaves_early() {
    assert_eq!(refusal(Some(&laptop_room(2.0))), None, "a host under its job target with memory for one build takes a run, however many cores its leases declare");
    let pressed = Room { admission: Admission { host_cpu_pressure_percent: 40.0, ..laptop_room(2.0).admission }, ..laptop_room(2.0) };
    let full = refusal(Some(&pressed)).expect("a host at the job target starts no other job");
    assert!(full.contains("CPU pressure 40% at the 40% job target"), "{full}");
    let starved = Room { measure: Measure { memory_available_mib: 9000.0, ..laptop_room(2.0).measure }, ..laptop_room(2.0) };
    assert!(refusal(Some(&starved)).is_some(), "memory under one job's peak and the host's floor holds it");
    let mut waiting = laptop_room(2.0);
    waiting.admission.land_check_waiting = true;
    assert!(refusal(Some(&waiting)).is_some());
    let status = serde_json::json!({ "profile": "unattended", "aggregateCpuLimit": 14.0, "committedBatchCpus": 2.0, "cpuSomeAvg10": 1.0, "heavySettlingSeconds": 11.0 });
    let settling = Room { admission: Admission::from_status(&status).expect("a status the taker reads"), ..laptop_room(2.0) };
    let why = refusal(Some(&settling)).expect("its admission defers a heavy run until the settle window ends");
    assert!(why.contains("settles for 11 s"), "{why}");
    assert!(refusal(None).is_some());
    let hosts = hosts();
    let lane = gather(&hosts.lane("left"), &hosts.laptop).unwrap().ship(&[]).unwrap();
    let command = ["cargo".to_owned(), "test".to_owned()];
    let job = Job { owner: "t", timeout_seconds: 60, command: &command, cpus: 6.0 };
    let (queued, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let refusing = std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(peer.try_clone().unwrap());
        std::io::BufRead::read_line(&mut reader, &mut String::new()).unwrap();
        Sender::to(peer).send(&crate::capacity::handover::Frame::NotSynced("the pulled lane is busy".to_owned()));
    });
    let refused = hand_over(queued, &lane, &job, None, &|_| panic!("a refused run never starts")).unwrap_err();
    refusing.join().unwrap();
    assert_eq!(refused, Some(Declined::Failed("the pulled lane is busy".to_owned())));
    let (queued, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    drop(peer);
    assert!(matches!(hand_over(queued, &lane, &job, None, &|_| panic!("never starts")), Err(Some(Declined::Failed(_)))));
}

fn host_room(limit: f64, committed: f64) -> Room {
    let mut room = laptop_room(committed);
    room.admission.limit_cpus = limit;
    room
}

#[test]
fn capacity_a_pulled_run_goes_to_the_target_with_the_most_free_cores_and_to_none_while_the_game_reserve_holds() {
    fn targets<'a>(laptop: &'a Room, ccx33: &'a Room, starting: usize) -> Vec<Target<'a>> {
        vec![Target { name: "whiterabbit", ssh: None, room: Some(laptop), starting: 0 }, Target { name: "ccx33", ssh: Some("tom@ccx33"), room: Some(ccx33), starting }]
    }
    let (laptop, ccx33) = (host_room(14.0, 2.0), host_room(8.0, 0.0));
    let busier = host_room(14.0, 8.0);
    assert_eq!(pick(&targets(&busier, &ccx33, 0), false), Ok(("ccx33", Some("tom@ccx33"))), "8 free cores on ccx33 beat 6 on the laptop");
    assert_eq!(pick(&targets(&busier, &ccx33, 1), false), Ok(("whiterabbit", None)), "a run already on its way to ccx33 counts against it");
    let reserve_full = host_room(14.0, 12.0);
    let pressed = Room { admission: Admission { host_cpu_pressure_percent: 40.0, ..reserve_full.admission }, ..reserve_full };
    let only_laptop = [Target { name: "whiterabbit", ssh: None, room: Some(&pressed), starting: 0 }];
    let refused = pick(&only_laptop, false).unwrap_err();
    assert!(refused[0].starts_with("whiterabbit: CPU pressure 40%"), "a host at its job target takes nothing, whatever its committed cores: {refused:?}");
    assert_eq!(pick(&targets(&laptop, &ccx33, 0), true), Err(vec!["this host holds its game reserve".to_owned()]));
}

#[test]
fn capacity_the_taker_never_rides_the_shared_ssh_master() {
    let offer = Offer {
        repo: "code/north/main".to_owned(),
        head: "a".repeat(40),
        name: "code-north-main-lane".to_owned(),
        subdirectory: String::new(),
        owner: "worker".to_owned(),
        timeout_seconds: 60,
        command: vec!["cargo".to_owned(), "test".to_owned()],
        bundle_bytes: 1,
        patch_bytes: 0,
        returned: "marker".to_owned(),
    };
    for command in [release_command("tom@gex45"), transport(&offer, Some("tom@ccx33"))] {
        let words: Vec<String> = command.get_args().map(|word| word.to_string_lossy().into_owned()).collect();
        assert_eq!(command.get_program(), "ssh");
        let control = words.iter().find_map(|word| word.strip_prefix("ControlPath=")).unwrap_or_default();
        assert!(control.ends_with("/north-taker-%C") && !control.contains("north-ssh-"), "{words:?}");
        assert!(words.windows(2).any(|pair| pair == ["-o", "ControlMaster=auto"]), "{words:?}");
    }
}

#[test]
fn capacity_the_takers_last_frame_reaches_the_far_end_before_it_hangs_up() {
    let scratch = tempfile::tempdir().unwrap();
    let received = scratch.path().join("received");
    let mut far = Command::new("sh").arg("-c").arg(format!("sleep 0.3; cat > {}", received.display())).stdin(std::process::Stdio::piped()).spawn().unwrap();
    let sender = std::sync::Arc::new(Sender::to(far.stdin.take().unwrap()));
    sender.send(&crate::capacity::handover::Frame::Exit(0));
    hang_up(far, sender);
    let wire = std::fs::read(&received).unwrap_or_default();
    assert_eq!(crate::capacity::handover::Frame::read(&mut wire.as_slice()).ok().flatten(), Some(crate::capacity::handover::Frame::Exit(0)), "{wire:?}");
}

#[test]
fn capacity_a_pulled_run_executes_in_a_sandbox_without_credentials_or_network_and_writes_only_its_lane() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    for secret in [".ssh/id_ed25519", ".config/sops/age/keys.txt", ".config/gh/hosts.yml"] {
        write(&home.join(secret), "secret\n");
    }
    let lane = home.join(".cache/build-offload/code-proj-main-pulled");
    std::fs::create_dir_all(&lane).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let probe = format!(
        "echo inside; test -e ~/.ssh && echo LEAK-ssh; test -e ~/.config/sops && echo LEAK-sops; test -e ~/.config/gh && echo LEAK-gh; [ -n \"${{GH_TOKEN-}}\" ] && echo LEAK-token; (exec 3<>/dev/tcp/127.0.0.1/{port}) 2>/dev/null && echo LEAK-net; touch written; echo done"
    );
    let north = std::env::current_exe().unwrap();
    let layout = crate::capacity::sandbox::Layout::of(&lane, &lane, &home, &north);
    let output = layout.command(&["bash".to_owned(), "-c".to_owned(), probe]).env("GH_TOKEN", "gho_secret").output().unwrap();
    let seen = String::from_utf8_lossy(&output.stdout);
    assert!(!seen.contains("LEAK"), "the sandboxed command reached what it must not: {seen}");
    if seen.contains("inside") {
        assert!(seen.contains("done") && lane.join("written").is_file(), "the lane stays writable: {seen} {}", String::from_utf8_lossy(&output.stderr));
    } else {
        assert!(!output.status.success() && !lane.join("written").exists(), "a host that cannot build the sandbox refuses the run: {}", String::from_utf8_lossy(&output.stderr));
    }
    assert!(super::remote::script(super::remote::Trust::Pulled).contains(r#""$north" capacity sandbox --lane "$slot" -- "$@""#), "the taker's lane script runs the job through the sandbox");
}

#[test]
fn capacity_a_pulled_run_finds_the_programs_its_host_keeps_in_a_home_profile() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let lane = home.join(".cache/build-offload/code-proj-main-pulled");
    std::fs::create_dir_all(&lane).unwrap();
    let git = std::env::split_paths(&std::env::var_os("PATH").unwrap()).map(|directory| directory.join("git")).find(|git| git.is_file()).expect("git on PATH");
    std::os::unix::fs::symlink(std::fs::canonicalize(git).unwrap().parent().unwrap(), home.join(".nix-profile")).unwrap();
    let north = std::env::current_exe().unwrap();
    let mut layout = crate::capacity::sandbox::Layout::of(&lane, &lane, &home, &north);
    if !layout.command(&["true".to_owned()]).status().is_ok_and(|status| status.success()) {
        return;
    }
    layout.path = Some(home.join(".nix-profile").into_os_string());
    let output = layout.command(&["git".to_owned(), "--version".to_owned()]).output().unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("git version"), "a pulled run finds no git its host keeps in a home profile: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn capacity_a_host_never_starts_an_offload_whose_announce_lost_to_the_gates_timeout() {
    let hosts = hosts();
    let lane = gather(&hosts.lane("abandoned"), &hosts.laptop).unwrap().ship(&[]).unwrap();
    let marker = hosts.remote.join("started");
    let stub = hosts.remote.join("north-stub");
    crate::foundation::shim::write_script(&stub, &format!("touch {}\n", marker.display()));
    let command = ["true".to_owned()];
    let (script, words) = invocation(&lane, &Job { owner: "t", timeout_seconds: 60, command: &command, cpus: 6.0 }, "NORTH-OFFLOAD-RETURNED-test");
    let mut transport = Command::new("sh");
    transport.args(["-c", &script, "offload"]).args(&words).env("HOME", &hosts.remote).env("AGENT_CAPACITY_NORTH", &stub);
    let ran = drive(transport, &lane, Some(std::fs::File::create(hosts.remote.join("output.log")).unwrap()), &|| false, "NORTH-OFFLOAD-RETURNED-test");
    assert!(matches!(ran, Ran::Abandoned));
    assert!(!marker.exists(), "the host started the command of an abandoned offer");
}

#[test]
fn capacity_a_pushed_build_waits_only_while_the_hosts_own_job_admission_refuses() {
    let room = laptop_room(12.0);
    assert!(matches!(room_for(&room), crate::capacity::build_offload::Room::Free { .. }), "2 of 14 cores free at no pressure still starts another build");
    let nearly = Room { admission: Admission { host_cpu_pressure_percent: 39.0, ..room.admission }, ..room };
    assert!(matches!(room_for(&nearly), crate::capacity::build_offload::Room::Free { .. }), "under the 40% job target the host admits one, so no build waits beside it");
    let pressed = Room { admission: Admission { host_cpu_pressure_percent: 40.0, ..room.admission }, ..room };
    assert!(matches!(room_for(&pressed), crate::capacity::build_offload::Room::Short(why) if why.contains("CPU pressure 40%")), "at the target the host holds new jobs");
    let starved = Room { measure: Measure { memory_available_mib: 9000.0, ..room.measure }, ..room };
    assert!(matches!(room_for(&starved), crate::capacity::build_offload::Room::Short(_)), "memory under one job's peak and the host's floor holds it");
}
