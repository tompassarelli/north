use std::fs;
use std::path::{Path, PathBuf};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use north_landing::land::candidate::{self, git};
use north_landing::land::jobs::Chores;
use north_landing::land::lander::Lander;
use north_landing::land::watch::Watch;
use north_landing::land::queue::{self, Ticket};
use north_landing::land::serve::Round;
use north_landing::land::{after, checks};

const CHILD: &str = "NORTH_LAND_CHAOS_HOME";
const LANDER_ROLE: &str = "NORTH_LAND_CHAOS_LANDER";
const STOPPED_AFTER_RUN: &str = "land_chaos::chaos_lander_stopped_by_sigterm_mid_after_run_reruns_it_and_reverts_the_red_lane";
const ROUNDS: usize = 200;
const SCENARIO_DEADLINE: Duration = Duration::from_secs(180);
const LANDER_LIFETIME: Duration = Duration::from_secs(120);

const CHECKER: &str = r#"plan="$1/plan"; calls="$1/calls"
shift
n=$(($(cat "$calls" 2>/dev/null || echo 0) + 1))
echo "$n" > "$calls"
step=$(sed -n "${n}p" "$plan" 2>/dev/null)
case "$step" in
lost) echo "ssh: connect to host gex45 port 22: Connection reset by peer" >&2; exit 255 ;;
slow) sleep 1; echo "safe-push: verdict green" ;;
*) echo "safe-push: verdict green" ;;
esac
"#;

const AFTER_RUNNER: &str = r#"[ "$1" = land ] || exit 0
eval outcome=\${$#}
while [ -e "$HOME/stall" ]; do echo $$ > "$HOME/stalled"; sleep 0.05; done
git rev-parse HEAD >> "$HOME/after-runs"
if [ -e bad.txt ]; then printf "event\tafter\tfailed\tbad.txt check\nfailed-step\tbad.txt check\t\n" > "$outcome"; exit 1; fi
"#;

fn isolated(name: &str, scenario: fn(&World)) {
    if let Some(home) = std::env::var_os(CHILD) {
        let home = Path::new(&home);
        scenario(&if std::env::var_os(LANDER_ROLE).is_some() { World::open(home) } else { World::new(home) });
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let gitconfig = home.path().join("gitconfig");
    fs::write(&gitconfig, "[init]\n\tdefaultBranch = main\n[commit]\n\tgpgsign = false\n[user]\n\tname = chaos\n\temail = chaos@example.com\n").unwrap();
    let north = home.path().join("north");
    north_base::foundation::shim::write_script(&north, AFTER_RUNNER);
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, home.path())
        .env("HOME", home.path())
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("NORTH_LAND_NORTH", &north)
        .env("NORTH_OFFLOAD_ROOT", home.path().join("offload"))
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env("GIT_CONFIG_NOSYSTEM", "1");
    let output = north_base::platform::process::try_run_group_traced_within(command, SCENARIO_DEADLINE)
        .unwrap()
        .unwrap_or_else(|processes| panic!("{name} ran past {SCENARIO_DEADLINE:?} in its isolated home: {processes}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success() && stdout.contains("1 passed"), "{name} failed in its isolated home:\n{stdout}\n{}", String::from_utf8_lossy(&output.stderr));
}

fn run(directory: &Path, arguments: &[&str]) -> String {
    git(directory, arguments).unwrap_or_else(|error| panic!("{error}"))
}

fn micros() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros()
}

struct Lane {
    name: String,
    subject: String,
}

struct World {
    root: PathBuf,
    origin: PathBuf,
    main: PathBuf,
    plan: PathBuf,
    checker: Vec<String>,
}

impl World {
    fn open(root: &Path) -> Self {
        let plan = root.join("checker");
        let script = root.join("checker.sh");
        let checker = vec!["sh".to_owned(), script.display().to_string(), plan.display().to_string()];
        Self { root: root.to_owned(), origin: root.join("origin.git"), main: root.join("proj/main"), plan, checker }
    }

    fn new(root: &Path) -> Self {
        let World { origin, main, plan, .. } = Self::open(root);
        fs::create_dir_all(&plan).unwrap();
        fs::create_dir_all(main.parent().unwrap()).unwrap();
        run(root, &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        run(root, &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
        fs::write(main.join("README"), "base\n").unwrap();
        run(&main, &["add", "-A"]);
        run(&main, &["commit", "-q", "-m", "base"]);
        run(&main, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
        fs::write(root.join("checker.sh"), CHECKER).unwrap();
        Self::open(root)
    }

    fn plan(&self, steps: &[&str]) {
        let _ = fs::remove_file(self.plan.join("calls"));
        fs::write(self.plan.join("plan"), steps.iter().map(|step| format!("{step}\n")).collect::<String>()).unwrap();
    }

    fn lander(&self) -> Lander {
        Lander::new(&self.main, self.checker.clone(), 1).unwrap()
    }

    fn lane_in(&self, checkout: &Path, root: &Path, branch: &str, file: &str) -> Ticket {
        run(checkout, &["fetch", "-q", "origin", "main"]);
        if checkout == root {
            run(root, &["checkout", "-q", "-B", branch, "FETCH_HEAD"]);
        } else {
            run(checkout, &["worktree", "add", "-q", "-b", branch, root.to_str().unwrap(), "FETCH_HEAD"]);
        }
        fs::write(root.join(file), format!("{branch}\n")).unwrap();
        run(root, &["add", "-A"]);
        run(root, &["commit", "-q", "-m", &format!("lane {branch} {file}")]);
        Ticket {
            origin: self.origin.display().to_string(),
            root: root.to_owned(),
            host: "laptop".into(),
            branch: branch.into(),
            head: run(root, &["rev-parse", "HEAD"]),
            session: None,
            queued_at: "2026-11-05T12:00:00Z".into(),
            ..Default::default()
        }
    }

    fn lane(&self, branch: &str) -> Ticket {
        let root = self.root.join("proj/worktrees").join(queue::slug(branch));
        self.lane_in(&self.main, &root, branch, &format!("{}.txt", queue::slug(branch)))
    }

    fn enqueue(&self, lander: &Lander, ticket: &Ticket) -> Lane {
        std::thread::sleep(Duration::from_millis(2));
        let name = queue::file_name(micros(), &ticket.branch);
        queue::write(&lander.queue.join(&name), ticket).unwrap();
        Lane { name, subject: run(&ticket.root, &["log", "-1", "--format=%s", &ticket.head]) }
    }

    fn landed_subjects(&self) -> Vec<String> {
        run(&self.origin, &["log", "--format=%s", "main"]).lines().map(str::to_owned).collect()
    }

    fn settled(&self, lander: &Lander, lanes: &[&Lane]) -> Vec<String> {
        let left: Vec<_> = queue::pending(&lander.queue).unwrap().into_iter().map(|(path, _)| path).collect();
        assert!(left.is_empty(), "lanes still queued: {left:?}");
        let subjects = self.landed_subjects();
        let main = run(&self.origin, &["rev-parse", "main"]);
        lanes
            .iter()
            .map(|lane| {
                let done = lander.queue.join("done").join(&lane.name);
                let verdict = queue::read(&done).unwrap_or_else(|error| panic!("{} was lost: {error}", lane.name)).verdict.unwrap_or_default();
                let copies = subjects.iter().filter(|subject| **subject == lane.subject).count();
                if let Some(sha) = verdict.strip_prefix("landed ") {
                    assert_eq!(copies, 1, "{} landed {copies} times: {subjects:?}", lane.name);
                    assert!(candidate::output(&self.origin, &["merge-base", "--is-ancestor", sha, &main]).unwrap().status.success(), "{} names {sha}, which is not on main", lane.name);
                } else {
                    assert!(verdict.starts_with("failed ") || verdict.starts_with("conflict "), "{} ended neither landed nor refused: {verdict}", lane.name);
                    assert_eq!(copies, 0, "{} was refused yet reached main", lane.name);
                }
                verdict
            })
            .collect()
    }
}

fn drive(lander: &Lander, state: &mut Watch) -> Vec<String> {
    let mut rounds = Vec::new();
    for _ in 0..ROUNDS {
        if queue::pending(&lander.queue).unwrap().is_empty() && state.in_flight() == 0 {
            return rounds;
        }
        let round = lander.round(state);
        if matches!(round, Ok(Round::Checking)) {
            std::thread::sleep(Duration::from_millis(20));
        }
        rounds.push(format!("{round:?}"));
    }
    panic!("the queue never drained: {:?}", &rounds[rounds.len().saturating_sub(5)..]);
}

fn landed(verdicts: &[String]) {
    assert!(verdicts.iter().all(|verdict| verdict.starts_with("landed ")), "{verdicts:?}");
}

fn kill_mid_check(world: &World) {
    world.plan(&["slow"]);
    let killed = world.lander();
    let a = world.enqueue(&killed, &world.lane("a"));
    let b = world.enqueue(&killed, &world.lane("b"));
    let base = candidate::prepare(&killed.repo, &killed.worktree).unwrap();
    let tickets = queue::collect(&killed.queue).unwrap();
    candidate::build(&killed.worktree, &base, &tickets, &[], usize::MAX, &killed.queue.join("logs")).unwrap();
    let check = checks::start(&checks::Launch::of(&killed), &killed.worktree, &base, &killed.queue.join("logs/killed.log"), true).unwrap();
    fs::write(killed.worktree.join("a.txt"), "half written\n").unwrap();
    fs::write(killed.worktree.join("stray"), "untracked\n").unwrap();
    drop(killed);
    let restarted = world.lander();
    drive(&restarted, &mut Watch::default());
    drop(check);
    landed(&world.settled(&restarted, &[&a, &b]));

    let c = world.enqueue(&restarted, &world.lane("c"));
    let base = candidate::prepare(&restarted.repo, &restarted.worktree).unwrap();
    let tickets = queue::collect(&restarted.queue).unwrap();
    let built = candidate::build(&restarted.worktree, &base, &tickets, &[], usize::MAX, &restarted.queue.join("logs")).unwrap();
    let included: Vec<_> = tickets.iter().map(|(path, ticket)| (path, ticket)).collect();
    after::begin(&restarted, &built.head, &base, &included, &built.tips).unwrap();
    run(&restarted.worktree, &["push", "-q", "origin", &format!("{}:refs/heads/main", built.head)]);
    drop(restarted);
    let again = world.lander();
    drive(&again, &mut Watch::default());
    landed(&world.settled(&again, &[&a, &b, &c]));
}

fn deleted_lander_worktrees(world: &World) {
    let first = world.lander();
    let a = world.enqueue(&first, &world.lane("a"));
    drive(&first, &mut Watch::default());
    fs::remove_dir_all(&first.worktree).unwrap();
    let second = world.lander();
    let b = world.enqueue(&second, &world.lane("b"));
    drive(&second, &mut Watch::default());
    landed(&world.settled(&second, &[&a, &b]));

    let third = world.lander();
    let c = world.enqueue(&third, &world.lane("c"));
    drive(&third, &mut Watch::default());
    fs::remove_dir_all(&third.worktree).unwrap();
    let fourth = world.lander();
    let d = world.enqueue(&fourth, &world.lane("d"));
    drive(&fourth, &mut Watch::default());
    landed(&world.settled(&fourth, &[&a, &b, &c, &d]));
}

fn duplicate_branch_names(world: &World) {
    let base = world.lander();
    let stray = Lander { worktree: world.root.join("proj/worktrees/worktrees/lander"), ..world.lander() };
    let early = world.enqueue(&stray, &world.lane("early"));
    let main = candidate::prepare(&stray.repo, &stray.worktree).unwrap();
    let held = candidate::build(&stray.worktree, &main, &queue::collect(&stray.queue).unwrap(), &[], usize::MAX, &stray.queue.join("logs")).unwrap().head;
    let stray_worktree = stray.worktree.clone();
    drop(stray);

    let clone = world.root.join("elsewhere/north");
    fs::create_dir_all(clone.parent().unwrap()).unwrap();
    run(&world.root, &["clone", "-q", world.origin.to_str().unwrap(), clone.to_str().unwrap()]);
    let here = world.enqueue(&base, &world.lane("fix"));
    let there = world.enqueue(&base, &world.lane_in(&clone, &clone, "fix", "fix-elsewhere.txt"));
    let slashed = world.enqueue(&base, &world.lane("feature/x"));
    let dashed = world.enqueue(&base, &world.lane_in(&world.main, &world.root.join("proj/worktrees/feature-x-dash"), "feature-x", "feature-x-dash.txt"));
    drive(&base, &mut Watch::default());
    landed(&world.settled(&base, &[&early, &here, &there, &slashed, &dashed]));
    assert_eq!(run(&stray_worktree, &["rev-parse", "HEAD"]), held, "another lander worktree's candidate moved under it");
}

fn remote_runner_lost_mid_check(world: &World) {
    world.plan(&["lost", "lost", "green"]);
    let lander = world.lander();
    let a = world.enqueue(&lander, &world.lane("a"));
    let b = world.enqueue(&lander, &world.lane("b"));
    let rounds = drive(&lander, &mut Watch::default());
    assert!(rounds.iter().filter(|round| round.contains("Waiting") && round.contains("transient")).count() >= 2, "{rounds:?}");
    landed(&world.settled(&lander, &[&a, &b]));

    world.plan(&["lost", "green", "green"]);
    let c = world.enqueue(&lander, &world.lane("c"));
    let d = world.enqueue(&lander, &world.lane("d"));
    drive(&lander, &mut Watch::default());
    landed(&world.settled(&lander, &[&a, &b, &c, &d]));
}

fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn journaled(lander: &Lander) -> usize {
    fs::read_dir(lander.queue.join("following")).map_or(0, |entries| entries.filter_map(Result::ok).filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json")).count())
}

fn verdicts(head: &str) -> Vec<String> {
    let ledger = PathBuf::from(std::env::var_os("XDG_STATE_HOME").unwrap()).join("safe-push").join("post-land.tsv");
    fs::read_to_string(ledger).unwrap_or_default().lines().filter_map(|line| {
        let fields: Vec<&str> = line.split('\t').collect();
        (fields.get(2) == Some(&head)).then(|| fields.get(3).copied().unwrap_or_default().to_owned())
    }).collect()
}

fn stopped_mid_after_run(world: &World) {
    if std::env::var_os(LANDER_ROLE).is_some() {
        let lander = world.lander();
        let mut watch = Watch::resume(&lander);
        let started = Instant::now();
        while started.elapsed() < LANDER_LIFETIME {
            let _ = lander.round(&mut watch);
            std::thread::sleep(Duration::from_millis(20));
        }
        return;
    }
    let lander = world.lander();
    let a = world.enqueue(&lander, &world.lane("a"));
    let bad = world.enqueue(&lander, &world.lane("bad"));
    let c = world.enqueue(&lander, &world.lane("c"));
    fs::write(world.root.join("stall"), "").unwrap();
    let mut first = Command::new(std::env::current_exe().unwrap()).args([STOPPED_AFTER_RUN, "--exact", "--nocapture", "--test-threads=1"]).env(LANDER_ROLE, "1").stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn().unwrap();
    wait_for("the first lander to land the batch and start its after run", || world.root.join("stalled").exists() && queue::pending(&lander.queue).unwrap().is_empty());
    let group = rustix::process::Pid::from_raw(first.id().cast_signed()).unwrap();
    rustix::process::kill_process_group(group, rustix::process::Signal::Term).unwrap();
    wait_for("the first lander to exit on SIGTERM", || first.try_wait().unwrap().is_some());
    let landed_head = run(&world.origin, &["rev-parse", "main"]);
    assert_eq!(journaled(&lander), 1, "the stopped lander left no journal entry for the after run of {landed_head}");
    assert!(!world.root.join("after-runs").exists(), "the after run finished before the lander was stopped");
    assert!(verdicts(&landed_head).is_empty(), "the stopped after run of {landed_head} was recorded as a verdict");

    fs::remove_file(world.root.join("stall")).unwrap();
    let restarted = world.lander();
    let mut watch = Watch::resume(&restarted);
    assert_eq!(watch.journaled(), 1);
    wait_for("the restarted lander to settle the rerun and land the revert", || {
        let _ = restarted.round(&mut watch);
        watch.in_flight() == 0 && queue::pending(&restarted.queue).unwrap().is_empty() && world.landed_subjects().iter().any(|subject| subject.starts_with("Revert "))
    });
    assert_eq!(journaled(&restarted), 0);
    assert_eq!(verdicts(&landed_head), ["red-main"], "the rerun after run of {landed_head} recorded no single red verdict");
    let runs = fs::read_to_string(world.root.join("after-runs")).unwrap();
    assert!(runs.lines().any(|head| head == landed_head), "the restarted lander never reran the after run of {landed_head}: {runs}");
    let subjects = world.landed_subjects();
    assert_eq!(subjects[0], format!("Revert {}", bad.subject), "{subjects:?}");
    assert_eq!(subjects.iter().filter(|subject| subject.starts_with("Revert ")).count(), 1, "{subjects:?}");
    for lane in [&a, &bad, &c] {
        assert_eq!(subjects.iter().filter(|subject| **subject == lane.subject).count(), 1, "{subjects:?}");
    }
    let culprit = run(&world.origin, &["log", "--format=%H %s", "main"]).lines().find_map(|line| line.split_once(' ').filter(|(_, subject)| *subject == bad.subject).map(|(sha, _)| sha.to_owned())).unwrap();
    let body = run(&world.origin, &["log", "-1", "--format=%B", "main"]);
    assert!(body.contains(&format!("Reverts: {culprit}")), "{body}");
    assert!(!run(&world.origin, &["ls-tree", "--name-only", "main"]).lines().any(|name| name == "bad.txt"));
}

#[test]
fn chaos_kill_mid_check_lands_each_lane_once_after_restart() {
    isolated("land_chaos::chaos_kill_mid_check_lands_each_lane_once_after_restart", kill_mid_check);
}

#[test]
fn chaos_lander_stopped_by_sigterm_mid_after_run_reruns_it_and_reverts_the_red_lane() {
    isolated(STOPPED_AFTER_RUN, stopped_mid_after_run);
}

#[test]
fn chaos_deleted_lander_worktrees_are_recreated() {
    isolated("land_chaos::chaos_deleted_lander_worktrees_are_recreated", deleted_lander_worktrees);
}

#[test]
fn chaos_duplicate_branch_names_land_as_separate_lanes() {
    isolated("land_chaos::chaos_duplicate_branch_names_land_as_separate_lanes", duplicate_branch_names);
}

#[test]
fn chaos_remote_runner_lost_mid_check_holds_then_lands() {
    isolated("land_chaos::chaos_remote_runner_lost_mid_check_holds_then_lands", remote_runner_lost_mid_check);
}


const STALL: Duration = Duration::from_secs(300);

fn stalled_patrol(_: &Lander) -> Result<(), String> {
    std::thread::sleep(STALL);
    Ok(())
}

fn stalled_recovery(_: &Lander, _: &Path) -> Result<Option<after::Recovered>, String> {
    std::thread::sleep(STALL);
    Ok(None)
}

fn stalled_pull(_: &Lander) -> Vec<String> {
    std::thread::sleep(STALL);
    Vec::new()
}

fn stalled_chores(world: &World) {
    let lander = Lander { chores: Chores { patrol: stalled_patrol, recover: stalled_recovery, pull: stalled_pull }, ..world.lander() };
    after::begin(&lander, "interrupted", "base", &[], &[]).unwrap();
    let a = world.enqueue(&lander, &world.lane("a"));
    let started = Instant::now();
    let mut watch = Watch::default();
    while !queue::pending(&lander.queue).unwrap().is_empty() {
        assert!(started.elapsed() < Duration::from_secs(30), "the queued lane waited 30 s behind the sheriff, the recovery and the outbox pull");
        let _ = lander.round(&mut watch);
        std::thread::sleep(Duration::from_millis(20));
    }
    landed(&world.settled(&lander, &[&a]));
    let mut running: Vec<&str> = watch.jobs.running().into_iter().map(|(kind, _)| kind.name()).collect();
    running.sort_unstable();
    assert_eq!(running, ["outbox pull", "recovery", "sheriff"], "the lane landed only because a stalled job never started");
}

#[test]
fn chaos_a_queued_lane_lands_while_the_sheriff_a_recovery_and_an_outbox_pull_each_stall_five_minutes() {
    isolated("land_chaos::chaos_a_queued_lane_lands_while_the_sheriff_a_recovery_and_an_outbox_pull_each_stall_five_minutes", stalled_chores);
}
