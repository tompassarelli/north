use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicU64;

use north_foundation::foundation::shim::write_executable;
use north_lander::land::heartbeat::Heartbeat;
use north_lander::land::jobs::Chores;
use north_lander::land::lander::Lander;
use north_lander::land::queue::{self, Ticket};

use super::land_first::{NO_SECRETS, PUBLIC_REPO, queueing};
use super::serve::{HANG, HOST, Repository, World, git, report};

const CHECKOUT: &str = "NORTH_LAND_IT_OUTBOX_CHECKOUT";
const REMOTE_SSH: &str = r#"#!/bin/sh
while [ $# -gt 0 ]; do case "$1" in -o|-p) shift 2 ;; -*) shift ;; *) break ;; esac; done
host=$1
shift
[ "$1" = -- ] && shift
printf '%s\n' "$host" >> "${FAKE_REMOTE_SSH_LOG:-/dev/null}"
REFUSED
c=$*
case "$host" in agent@*) export XDG_STATE_HOME="$FAKE_AGENT_STATE"; c=$(printf '%s' "$c" | sed "s#/home/agent/#$FAKE_AGENT_HOME/#g") ;; *) export XDG_STATE_HOME="$FAKE_REMOTE_STATE" ;; esac
exec sh -c "$c"
"#;

fn remote_ssh(refused: &str) -> String {
    REMOTE_SSH.replace("REFUSED", refused)
}

#[test]
fn outbox_puller() {
    let Ok(checkout) = std::env::var(CHECKOUT) else { return };
    let checkout = PathBuf::from(checkout);
    let origin = git(&checkout, &["remote", "get-url", "--push", "origin"]);
    let queue = queue::directory(&PathBuf::from(std::env::var("XDG_STATE_HOME").unwrap()), &origin);
    let lander = Lander { repo: checkout.clone(), worktree: checkout, offload_owner: String::new(), queue, checker: Vec::new(), app_token: Vec::new(), round: AtomicU64::new(0), chores: Chores::default(), heartbeat: Heartbeat::default(), wake: std::sync::Arc::default() };
    let pulled = lander.pull_remote();
    for waiting in pulled.waiting.iter().chain(&lander.settle(&pulled.taken)) {
        println!("waiting: {waiting}");
    }
}

fn outboxed(outbox: &Path, micros: u128, root: &Path, branch: &str, head: &str) -> String {
    let name = queue::file_name(micros, branch);
    let ticket = Ticket { origin: "https://github.com/tompassarelli/north".into(), root: root.to_owned(), host: "gex45".into(), branch: branch.into(), head: head.into(), session: Some(format!("{branch}-session")), queued_at: "2026-11-07T03:07:09Z".into(), queued_micros: Some(micros), ..Ticket::default() };
    queue::write(&outbox.join(&name), &ticket).unwrap();
    name
}

struct Pull {
    world: World,
    repository: Repository,
    remote: PathBuf,
    calls: PathBuf,
    key: String,
}

impl Pull {
    fn new() -> Self {
        Self::declaring("[hosts.gex45]\nssh = \"gex45\"\n")
    }

    fn declaring(hosts: &str) -> Self {
        let world = World::new();
        let repository = world.repository("north");
        write_executable(&world.path().join("bin/ssh"), remote_ssh(""));
        let fleet = world.path().join("code/south/main/agents");
        std::fs::create_dir_all(&fleet).unwrap();
        std::fs::write(fleet.join("orchestration.toml"), hosts).unwrap();
        Self { remote: world.path().join("remote-state"), calls: world.path().join("ssh.calls"), key: repository.queue.file_name().unwrap().to_string_lossy().into_owned(), repository, world }
    }

    fn process(&self, name: &str) -> Command {
        let path = format!("{}:{}", self.world.path().join("bin").display(), std::env::var("PATH").unwrap_or_default());
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", name, "--nocapture", "--test-threads", "1"])
            .env(CHECKOUT, &self.repository.checkout)
            .env("PATH", path)
            .env("HOME", self.world.path())
            .env("XDG_STATE_HOME", self.world.path().join("state"))
            .env("FAKE_REMOTE_STATE", &self.remote)
            .env("FAKE_AGENT_STATE", self.agent_state())
            .env("FAKE_AGENT_HOME", self.world.path().join("agent-root"))
            .env("FAKE_REMOTE_SSH_LOG", &self.calls)
            .env("NORTH_LAND_NORTH", self.world.path().join("bin/north"))
            .env("GIT_CONFIG_GLOBAL", self.world.path().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("NORTH_TEST_ROOT")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn outbox(&self) -> PathBuf {
        self.remote.join("north/land-queue").join(&self.key).join(queue::OUTBOX)
    }

    fn agent_state(&self) -> PathBuf {
        self.world.path().join("agent-state")
    }

    fn agent_outbox(&self) -> PathBuf {
        self.agent_state().join("north/land-queue").join(&self.key).join(queue::OUTBOX)
    }

    fn lane_outboxed(&self) -> String {
        let lane = self.repository.checkout.parent().unwrap().join("worktrees/behind");
        git(&self.repository.checkout, &["worktree", "add", "-q", "-b", "behind", lane.to_str().unwrap()]);
        git(&lane, &["commit", "-q", "--allow-empty", "-m", "behind"]);
        outboxed(&self.outbox(), 1_794_020_900_000_000, &lane, "behind", &git(&lane, &["rev-parse", "HEAD"]))
    }

    fn logins(&self) -> Vec<String> {
        std::fs::read_to_string(&self.calls).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    fn pull(&self) -> String {
        let output = self.process("outbox_pull::outbox_puller").output().unwrap();
        format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
    }
}

const AGENT_LOGIN: &str = "[hosts.gex45]\nssh = \"tom@gex45\"\nagent_ssh = \"agent@gex45\"\n";

#[test]
fn a_lane_under_the_agent_users_home_is_read_fetched_and_settled_through_the_agent_login_and_an_operator_lane_on_the_same_host_through_the_operator_login() {
    let empty = Pull::declaring(AGENT_LOGIN);
    let transcript = empty.pull();
    let mut dialed = empty.logins();
    dialed.sort();
    dialed.dedup();
    assert_eq!(dialed, ["agent@gex45", "tom@gex45"], "{transcript}");
    assert!(!empty.remote.join("north/land-queue").join(&empty.key).exists() && !empty.agent_state().join("north/land-queue").join(&empty.key).exists(), "a host with no outbox for {} gained its queue:\n{transcript}", empty.key);

    let pull = Pull::declaring(AGENT_LOGIN);
    let operator = pull.lane_outboxed();
    let agent_root = pull.world.path().join("agent-root/code/north");
    git(pull.world.path(), &["clone", "-q", pull.repository.origin.to_str().unwrap(), agent_root.to_str().unwrap()]);
    git(&agent_root, &["checkout", "-q", "-b", "agent-lane"]);
    let head = commit_in(&agent_root, "agent lane");
    let agent_outbox = pull.agent_outbox();
    let agent = outboxed(&agent_outbox, 1_794_020_950_000_000, Path::new("/home/agent/code/north"), "agent-lane", &head);
    let astray = outboxed(&agent_outbox, 1_794_020_955_000_000, Path::new("/home/agent/code/north"), "agent-lane", &head);
    let elsewhere = Ticket { host: "cpx62".into(), ..queue::read(&agent_outbox.join(&astray)).unwrap() };
    queue::write(&agent_outbox.join(&astray), &elsewhere).unwrap();
    let operator_lane = pull.repository.checkout.parent().unwrap().join("worktrees/behind");
    let borrowed = outboxed(&agent_outbox, 1_794_020_960_000_000, &operator_lane, "behind", &git(&operator_lane, &["rev-parse", "HEAD"]));
    let transcript = pull.pull();

    assert!(pull.repository.queue.join(&operator).is_file() && pull.outbox().join("taken").join(&operator).is_file(), "the operator's lane was not taken and settled through the operator login:\n{transcript}");
    assert!(pull.repository.queue.join(&agent).is_file() && agent_outbox.join("taken").join(&agent).is_file(), "the agent's lane was not taken and settled through the agent login:\n{transcript}");
    assert_eq!(git(&pull.repository.checkout.parent().unwrap().join("trusted/repo.git"), &["rev-parse", "refs/land-remote/agent@gex45/agent-lane"]), head, "{transcript}");
    assert!(!pull.repository.queue.join(&astray).exists() && agent_outbox.join(queue::WITHDRAWN).join(&astray).is_file() && pull.logins().iter().all(|login| login.ends_with("@gex45")), "a ticket in gex45's agent outbox naming another host was taken or reached that host: {:?}\n{transcript}", pull.logins());
    assert!(pull.remote.join("north/land-queue").join(&pull.key).join(queue::LANDER_LEDGER).is_file() && !pull.agent_state().join("north/land-queue").join(&pull.key).join(queue::LANDER_LEDGER).exists(), "the lander's ledger was mirrored into the agent user's home:\n{transcript}");
    assert!(!pull.repository.queue.join(&borrowed).exists() && agent_outbox.join(queue::WITHDRAWN).join(&borrowed).is_file(), "a ticket in the agent's outbox naming the operator's lane was taken:\n{transcript}");
}

#[test]
fn an_ask_from_the_agent_users_outbox_about_an_operator_ticket_is_answered_as_unknown_and_the_operators_own_ask_with_its_place() {
    let pull = Pull::declaring(AGENT_LOGIN);
    let lane = pull.repository.checkout.parent().unwrap().join("worktrees/behind");
    git(&pull.repository.checkout, &["worktree", "add", "-q", "-b", "behind", lane.to_str().unwrap()]);
    let head = commit_in(&lane, "behind");
    let name = outboxed(&pull.repository.queue, 1_794_020_900_000_000, &lane, "behind", &head);
    for outbox in [pull.outbox(), pull.agent_outbox()] {
        std::fs::create_dir_all(outbox.join(queue::ASKS)).unwrap();
        std::fs::write(outbox.join(queue::ASKS).join(&name), "").unwrap();
    }
    let transcript = pull.pull();

    let told = |outbox: PathBuf| std::fs::read_to_string(outbox.join(queue::ANSWERS).join(&name)).unwrap_or_else(|error| panic!("{error}\n{transcript}"));
    assert!(told(pull.outbox()).contains(&format!("behind ({head})")), "the operator's own ask lost its ticket's place:\n{transcript}");
    let agent = told(pull.agent_outbox());
    assert!(agent.contains(&format!("holds no ticket {name}")) && !agent.contains(&head), "the agent user's outbox was told about the operator's ticket: {agent}\n{transcript}");
}

fn commit_in(repository: &Path, message: &str) -> String {
    git(repository, &["commit", "-q", "--allow-empty", "-m", message]);
    git(repository, &["rev-parse", "HEAD"])
}

fn bundled(outbox: &Path, ticket: &str, repository: &Path, branch: &str) {
    let bundle = queue::bundle(&outbox.join(ticket));
    git(repository, &["bundle", "create", "-q", bundle.to_str().unwrap(), &format!("refs/heads/{branch}"), "^origin/main"]);
}

#[test]
fn a_lane_is_taken_as_the_objects_of_its_bundle_whatever_its_repository_configures_and_a_head_the_bundle_lacks_is_not_taken() {
    let pull = Pull::declaring(AGENT_LOGIN);
    write_executable(&pull.world.path().join("bin/ssh"), remote_ssh(r#"case "$*" in *git-upload-pack*) exit 255 ;; esac"#));
    let agent = pull.world.path().join("agent-home/north");
    git(pull.world.path(), &["clone", "-q", pull.repository.origin.to_str().unwrap(), agent.to_str().unwrap()]);
    let outbox = pull.outbox();
    std::fs::create_dir_all(&outbox).unwrap();
    git(&agent, &["checkout", "-q", "-b", "whole"]);
    let whole = commit_in(&agent, "whole");
    let whole_ticket = queue::file_name(1_794_020_900_000_000, "whole");
    bundled(&outbox, &whole_ticket, &agent, "whole");
    outboxed(&outbox, 1_794_020_900_000_000, &agent, "whole", &whole);
    git(&agent, &["checkout", "-q", "-b", "stale", "origin/main"]);
    commit_in(&agent, "bundled");
    let stale_ticket = queue::file_name(1_794_020_900_000_001, "stale");
    bundled(&outbox, &stale_ticket, &agent, "stale");
    let stale_head = commit_in(&agent, "after the bundle");
    outboxed(&outbox, 1_794_020_900_000_001, &agent, "stale", &stale_head);

    git(&agent, &["checkout", "-q", "-b", "borrowed", "origin/main"]);
    commit_in(&agent, "its own");
    let borrowed_ticket = queue::file_name(1_794_020_900_000_002, "borrowed");
    bundled(&outbox, &borrowed_ticket, &agent, "borrowed");
    outboxed(&outbox, 1_794_020_900_000_002, &agent, "borrowed", &whole);

    let marker = pull.world.path().join("ran-in-the-agents-repository");
    let hostile = pull.world.path().join("hostile");
    write_executable(&hostile, format!("#!/bin/sh\ntouch {}\n", marker.display()));
    let hooks = pull.world.path().join("hostile-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    for hook in ["reference-transaction", "post-checkout", "post-merge", "pre-auto-gc"] {
        write_executable(&hooks.join(hook), format!("#!/bin/sh\ntouch {}\n", marker.display()));
    }
    for (key, value) in [("core.hooksPath", hooks.to_str().unwrap()), ("core.fsmonitor", hostile.to_str().unwrap()), ("core.sshCommand", hostile.to_str().unwrap()), ("core.pager", hostile.to_str().unwrap()), ("uploadpack.packObjectsHook", hostile.to_str().unwrap()), ("filter.hostile.clean", hostile.to_str().unwrap()), ("diff.external", hostile.to_str().unwrap())] {
        git(&agent, &["config", key, value]);
    }
    let transcript = pull.pull();

    assert!(pull.repository.queue.join(&whole_ticket).is_file(), "the lane its bundle holds was not taken:\n{transcript}");
    assert_eq!(git(&pull.repository.checkout.parent().unwrap().join("trusted/repo.git"), &["rev-parse", "refs/land-remote/gex45/whole"]), whole, "{transcript}");
    assert!(transcript.contains("north land: took whole from gex45's bundle"), "{transcript}");
    assert!(!pull.repository.queue.join(&stale_ticket).exists() && outbox.join(&stale_ticket).is_file(), "a ticket whose head no fetched object holds was queued:\n{transcript}");
    assert!(!pull.repository.queue.join(&borrowed_ticket).exists() && outbox.join(&borrowed_ticket).is_file(), "a ticket naming another lane's commit was queued:\n{transcript}");
    assert!(!marker.exists(), "git ran what the agent's repository configures:\n{transcript}");
}

const READY: &str = "NORTH_LAND_IT_VERDICTS_READY";
const WAITED: &str = "NORTH_LAND_IT_WAITED_TICKET";

#[test]
fn verdict_sender() {
    let (Ok(checkout), Ok(ready)) = (std::env::var(CHECKOUT), std::env::var(READY)) else { return };
    let checkout = PathBuf::from(checkout);
    let origin = git(&checkout, &["remote", "get-url", "--push", "origin"]);
    let queue = queue::directory(&PathBuf::from(std::env::var("XDG_STATE_HOME").unwrap()), &origin);
    let lander = Lander { repo: checkout.clone(), worktree: checkout, offload_owner: String::new(), queue, checker: Vec::new(), app_token: Vec::new(), round: AtomicU64::new(0), chores: Chores::default(), heartbeat: Heartbeat::default(), wake: std::sync::Arc::default() };
    let _sending = north_lander::land::serve::verdicts::watch(&lander);
    std::fs::write(&ready, "").unwrap();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new());
}

#[test]
fn hand_in_waiter() {
    let Ok(path) = std::env::var(WAITED) else { return };
    let path = PathBuf::from(path);
    let ticket = queue::read(&path).unwrap();
    let queued = north_lander::land::enqueue::Queued { path, outbox: true, ticket: ticket.clone(), squash: north_landing::landing::lane_squash::Squash::Unchanged };
    std::process::exit(i32::from(north_lander::land::enqueue::hand_in(&ticket.root, &queued, &PathBuf::from(std::env::var("XDG_STATE_HOME").unwrap()))));
}

#[test]
fn a_refusal_reaches_the_hand_in_on_the_lanes_host_by_the_landers_push_with_no_records_mirror() {
    let pull = Pull::new();
    pull.repository.lane("conflicted");
    let (lander_copy, ticket) = queue::pending(&pull.repository.queue).unwrap().pop().unwrap();
    let ticket = Ticket { host: "gex45".into(), ..ticket };
    queue::write(&lander_copy, &ticket).unwrap();
    north_base::land_ticket::queued(&lander_copy, "taken from gex45's outbox").unwrap();
    let outboxed = queue::directory(&pull.remote, &ticket.origin).join(queue::OUTBOX).join(lander_copy.file_name().unwrap());
    queue::write(&outboxed, &ticket).unwrap();

    let ready = pull.world.path().join("verdicts.ready");
    let mut sender = pull.process("outbox_pull::verdict_sender");
    sender.env(READY, &ready).stdin(Stdio::piped());
    let mut sender = sender.spawn().unwrap();
    let (lines, sent) = std::sync::mpsc::channel();
    let reader = std::io::BufReader::new(sender.stdout.take().unwrap());
    std::thread::spawn(move || std::io::BufRead::lines(reader).map_while(Result::ok).for_each(|line| { let _ = lines.send(line); }));
    let mut waiter = pull.process("outbox_pull::hand_in_waiter");
    waiter.env(WAITED, &outboxed).env("XDG_STATE_HOME", &pull.remote);
    let mut waiter = waiter.spawn().unwrap();
    north_base::platform::wait::until(HANG, || ready.exists() || sender.try_wait().unwrap().is_some());
    assert!(ready.exists(), "the lander never started sending verdicts");

    let verdict = "conflict with main at conflicted.txt; rebase onto origin/main and hand in again";
    queue::decide(&lander_copy, ticket, verdict.to_owned()).unwrap();
    let mut journal = Vec::new();
    let pushed = sent.iter().inspect(|line| journal.push(line.clone())).any(|line| line.contains("sent the verdict on"));
    north_base::platform::wait::until(HANG, || waiter.try_wait().unwrap().is_some());
    let answered = waiter.try_wait().unwrap().is_some();
    if !answered {
        let _ = waiter.kill();
    }
    let _ = sender.kill();
    let _ = sender.wait();
    let output = waiter.wait_with_output().unwrap();
    let transcript = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(pushed, "the lander sent no verdict to gex45 on the decision event:\n{}", journal.join("\n"));
    assert!(answered, "the hand-in on gex45 never heard the lander's refusal:\n{transcript}");
    assert!(output.status.code() == Some(1) && transcript.contains(verdict), "{transcript}");
}

#[test]
fn remote_lane_lands_through_the_landers_pull_of_its_hosts_outbox_with_no_patch_carried() {
    let here = north_base::platform::land_queue::local_host();
    let pull = Pull::declaring(&format!("[hosts.\"{here}\"]\nssh = \"{here}\"\n"));
    let bin = pull.world.path().join("bin");
    write_executable(&bin.join("judge"), "#!/bin/sh\ncat > /dev/null\nexit 0\n");
    write_executable(&bin.join("betterleaks"), NO_SECRETS);
    write_executable(&bin.join("gh"), PUBLIC_REPO);
    std::fs::write(pull.world.path().join("denylist.toml"), "[[rules]]\nid = \"fixture\"\n").unwrap();
    std::fs::write(pull.world.path().join("private-kinds.toml"), "[[kind]]\nid = \"address\"\n").unwrap();
    let south = pull.world.path().join("code/south/main");
    git(pull.world.path(), &["init", "-q", south.to_str().unwrap()]);
    git(&south, &["add", "-A"]);
    git(&south, &["commit", "-q", "-m", "south"]);
    let main = &pull.repository.checkout;
    std::fs::write(main.join(".safe-push"), format!("[landing]\n\tpush-app = lander\n\tlander-host = {HOST}\n")).unwrap();
    git(main, &["add", "-A"]);
    git(main, &["commit", "-q", "-m", "Name the lander host"]);
    git(main, &["push", "-q", "origin", "HEAD:refs/heads/main"]);

    let build_host = pull.world.path().join("gex45/north");
    git(pull.world.path(), &["clone", "-q", pull.repository.origin.to_str().unwrap(), build_host.to_str().unwrap()]);
    git(&build_host, &["checkout", "-q", "-b", "remote-lane"]);
    std::fs::write(build_host.join("finished.txt"), "committed on a build host\n").unwrap();
    git(&build_host, &["add", "-A"]);
    git(&build_host, &["commit", "-q", "-m", "Finish on a build host"]);

    let mut enqueue = queueing(&pull.world, "serve::enqueue_process", "NORTH_LAND_IT_ARGUMENTS", &["--repo", build_host.to_str().unwrap()]);
    enqueue.env("XDG_STATE_HOME", &pull.remote).env("RESIDENT_CAP_HOST", &here);
    let queued = enqueue.output().unwrap();
    let text = report(&queued);
    assert!(queued.status.success() && text.contains("Done: queued (outbox)"), "{text}");
    assert!(pull.outbox().read_dir().unwrap().flatten().any(|entry| entry.path().extension().is_some_and(|extension| extension == "bundle")), "the build host wrote no bundle: {text}");

    let mut serve = pull.world.serve_command(&pull.repository, None);
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    serve.env("PATH", path).env("FAKE_REMOTE_STATE", &pull.remote).env("FAKE_AGENT_STATE", pull.agent_state()).env("FAKE_AGENT_HOME", pull.world.path().join("agent-root"));
    let served = serve.spawn().unwrap().wait_with_output().unwrap();
    let journal = report(&served);
    let subjects = git(&pull.repository.origin, &["log", "--format=%s", "main"]);
    assert!(subjects.lines().any(|subject| subject == "Finish on a build host"), "the lane committed on the build host did not land:\n{journal}\n{subjects}");
    assert!(pull.outbox().join("taken").read_dir().is_ok_and(|taken| taken.flatten().next().is_some()), "the build host's ticket was not settled:\n{journal}");
}
