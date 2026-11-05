use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use north_landing::land::candidate::{self, git};
use north_landing::land::lander::Lander;
use north_landing::land::precheck::State;
use north_landing::land::queue::{self, Ticket};
use north_landing::land::serve::Round;
use north_landing::land::{after, checks};

const CHILD: &str = "NORTH_LAND_CHAOS_HOME";
const ROUNDS: usize = 200;

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

fn isolated(name: &str, scenario: fn(&World)) {
    if let Some(home) = std::env::var_os(CHILD) {
        scenario(&World::new(Path::new(&home)));
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let gitconfig = home.path().join("gitconfig");
    fs::write(&gitconfig, "[init]\n\tdefaultBranch = main\n[commit]\n\tgpgsign = false\n[user]\n\tname = chaos\n\temail = chaos@example.com\n").unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, home.path())
        .env("HOME", home.path())
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("NORTH_LAND_NORTH", "true")
        .env("NORTH_OFFLOAD_ROOT", home.path().join("offload"))
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
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
    fn new(root: &Path) -> Self {
        let origin = root.join("origin.git");
        let main = root.join("proj/main");
        let plan = root.join("checker");
        fs::create_dir_all(&plan).unwrap();
        fs::create_dir_all(main.parent().unwrap()).unwrap();
        run(root, &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        run(root, &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
        fs::write(main.join("README"), "base\n").unwrap();
        run(&main, &["add", "-A"]);
        run(&main, &["commit", "-q", "-m", "base"]);
        run(&main, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
        let script = root.join("checker.sh");
        fs::write(&script, CHECKER).unwrap();
        let checker = vec!["sh".to_owned(), script.display().to_string(), plan.display().to_string()];
        Self { root: root.to_owned(), origin, main, plan, checker }
    }

    fn plan(&self, steps: &[&str]) {
        let _ = fs::remove_file(self.plan.join("calls"));
        fs::write(self.plan.join("plan"), steps.iter().map(|step| format!("{step}\n")).collect::<String>()).unwrap();
    }

    fn slots(&self, count: usize) {
        run(&self.main, &["config", "landing.slots", &count.to_string()]);
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

fn drive(lander: &Lander, state: &mut State) -> Vec<String> {
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
    let check = checks::start(&checks::Launch::of(&killed), &killed.worktree, &base, &killed.queue.join("logs/killed.log"), true, None).unwrap();
    fs::write(killed.worktree.join("a.txt"), "half written\n").unwrap();
    fs::write(killed.worktree.join("stray"), "untracked\n").unwrap();
    drop(killed);
    let restarted = world.lander();
    drive(&restarted, &mut State::default());
    drop(check);
    landed(&world.settled(&restarted, &[&a, &b]));

    let c = world.enqueue(&restarted, &world.lane("c"));
    let base = candidate::prepare(&restarted.repo, &restarted.worktree).unwrap();
    let tickets = queue::collect(&restarted.queue).unwrap();
    let built = candidate::build(&restarted.worktree, &base, &tickets, &[], usize::MAX, &restarted.queue.join("logs")).unwrap();
    let included: Vec<_> = tickets.iter().map(|(path, ticket)| (path, ticket)).collect();
    after::begin(&restarted, &built.head, &base, &included).unwrap();
    run(&restarted.worktree, &["push", "-q", "origin", &format!("{}:refs/heads/main", built.head)]);
    drop(restarted);
    let again = world.lander();
    drive(&again, &mut State::default());
    landed(&world.settled(&again, &[&a, &b, &c]));
}

fn restart_while_draining(world: &World) {
    world.slots(2);
    world.plan(&["slow", "slow", "slow", "slow"]);
    let retiring = world.lander();
    let a = world.enqueue(&retiring, &world.lane("a"));
    let b = world.enqueue(&retiring, &world.lane("b"));
    let c = world.enqueue(&retiring, &world.lane("c"));
    let mut state = State::default();
    assert_eq!(retiring.round(&mut state), Ok(Round::Checking));
    assert_eq!(state.drain(), 2);
    for _ in 0..ROUNDS {
        if state.in_flight() == 0 {
            break;
        }
        let _ = retiring.round(&mut state);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(state.in_flight(), 0);
    let left: Vec<_> = queue::pending(&retiring.queue).unwrap().into_iter().map(|(_, ticket)| ticket.branch).collect();
    assert_eq!(left, ["b", "c"], "a draining lander starts no lane, lands the one whose check still matches main, and leaves the lane main moved under queued for its successor to check again");

    let d = world.enqueue(&retiring, &world.lane("d"));
    let restarted = world.lander();
    let mut cut = State::default();
    assert_eq!(restarted.round(&mut cut), Ok(Round::Checking));
    cut.drain();
    drop(cut);
    drop(restarted);
    let fresh = world.lander();
    drive(&fresh, &mut State::default());
    landed(&world.settled(&fresh, &[&a, &b, &c, &d]));
}

fn deleted_lander_worktrees(world: &World) {
    let first = world.lander();
    let a = world.enqueue(&first, &world.lane("a"));
    drive(&first, &mut State::default());
    fs::remove_dir_all(&first.worktree).unwrap();
    let second = world.lander();
    let b = world.enqueue(&second, &world.lane("b"));
    drive(&second, &mut State::default());
    landed(&world.settled(&second, &[&a, &b]));

    world.slots(1);
    let third = world.lander();
    let c = world.enqueue(&third, &world.lane("c"));
    drive(&third, &mut State::default());
    let precheck = candidate::lander_worktree(&third.repo, "lander-precheck-0").unwrap();
    assert!(precheck.join(".git").exists());
    fs::remove_dir_all(&precheck).unwrap();
    fs::remove_dir_all(&third.worktree).unwrap();
    let fourth = world.lander();
    let d = world.enqueue(&fourth, &world.lane("d"));
    drive(&fourth, &mut State::default());
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
    drive(&base, &mut State::default());
    landed(&world.settled(&base, &[&early, &here, &there, &slashed, &dashed]));
    assert_eq!(run(&stray_worktree, &["rev-parse", "HEAD"]), held, "another lander worktree's candidate moved under it");
}

fn remote_runner_lost_mid_check(world: &World) {
    world.plan(&["lost", "lost", "green"]);
    let lander = world.lander();
    let a = world.enqueue(&lander, &world.lane("a"));
    let b = world.enqueue(&lander, &world.lane("b"));
    let rounds = drive(&lander, &mut State::default());
    assert!(rounds.iter().filter(|round| round.contains("Waiting") && round.contains("transient")).count() >= 2, "{rounds:?}");
    landed(&world.settled(&lander, &[&a, &b]));

    world.slots(2);
    world.plan(&["lost", "green", "green"]);
    let c = world.enqueue(&lander, &world.lane("c"));
    let d = world.enqueue(&lander, &world.lane("d"));
    drive(&lander, &mut State::default());
    landed(&world.settled(&lander, &[&a, &b, &c, &d]));
}

const COMPILER: &str = r#"echo "recheck=${NORTH_LAND_RECHECK:-}" >> "$HOME/compiler.calls"
if [ -e builder.txt ] && grep -q breaches record.txt 2>/dev/null; then
echo "error[E0063]: missing field breaches in initializer of Record" >&2
echo "safe-push: verdict refused E0063"
exit 1
fi
echo "safe-push: verdict green"
"#;

fn file_disjoint_lanes_that_break_each_other(world: &World) {
    world.slots(2);
    let script = world.root.join("compiler.sh");
    fs::write(&script, COMPILER).unwrap();
    let lander = Lander::new(&world.main, vec!["sh".to_owned(), script.display().to_string()], 1).unwrap();
    let builder = world.enqueue(&lander, &world.lane_in(&world.main, &world.root.join("proj/worktrees/builder"), "builder", "builder.txt"));
    let mut field = world.lane_in(&world.main, &world.root.join("proj/worktrees/field"), "field", "record.txt");
    fs::write(field.root.join("record.txt"), "since sent breaches\n").unwrap();
    run(&field.root, &["commit", "-q", "-am", "lane field adds breaches to Record"]);
    field.head = run(&field.root, &["rev-parse", "HEAD"]);
    let field = world.enqueue(&lander, &field);
    drive(&lander, &mut State::default());
    let verdicts = world.settled(&lander, &[&builder, &field]);
    assert!(verdicts[0].starts_with("landed "), "{verdicts:?}");
    assert!(verdicts[1].starts_with("failed "), "a lane checked on an older main landed beside a change it breaks: {verdicts:?}");
    let calls = fs::read_to_string(world.root.join("compiler.calls")).unwrap();
    assert_eq!(calls.lines().filter(|call| *call == "recheck=1").count(), 1, "the lane main moved under, with no file in common, is rechecked with the recheck lines only: {calls}");
}

#[test]
fn chaos_kill_mid_check_lands_each_lane_once_after_restart() {
    isolated("land_chaos::chaos_kill_mid_check_lands_each_lane_once_after_restart", kill_mid_check);
}

#[test]
fn chaos_restart_while_draining_loses_no_lane() {
    isolated("land_chaos::chaos_restart_while_draining_loses_no_lane", restart_while_draining);
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

#[test]
fn chaos_file_disjoint_lanes_that_break_each_other_are_rechecked_on_the_main_they_land_on() {
    isolated("land_chaos::chaos_file_disjoint_lanes_that_break_each_other_are_rechecked_on_the_main_they_land_on", file_disjoint_lanes_that_break_each_other);
}
