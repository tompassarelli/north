use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use north_lander::land::queue;

use super::serve::{Repository, World, git, report, runtime_commit};

const COUNTED: &str = "#!/bin/sh\npwd >> \"$0.calls\"\necho 'safe-push: verdict green'\n";
const AFTER_RUNNER: &str = r#"#!/bin/sh
[ "$1" = land ] && [ "$2" = checks ] || exit 0
eval outcome=\${$#}
tree=; head=; previous=
for argument in "$@"; do
  [ "$previous" = --tree ] && tree=$argument
  [ "$previous" = --head ] && head=$argument
  previous=$argument
done
[ -n "$tree" ] && [ "$tree" != "$head" ] && echo "$*" >> "$0.probes"
failed=
[ -e bad.txt ] && failed="$failed bad.txt"
[ -e worse.txt ] && failed="$failed worse.txt"
[ -z "$failed" ] && exit 0
for file in $failed; do
  printf "event\tafter\tfailed\t%s check\nfailed-step\t%s check\t\n" "$file" "$file" >> "$outcome"
done
exit 1
"#;

fn checker(world: &World) -> std::path::PathBuf {
    let path = world.path().join("bin/checker");
    north_base::foundation::shim::write_executable(&path, COUNTED);
    world.path().join("bin/checker.calls")
}

fn calls(path: &Path) -> usize {
    fs::read_to_string(path).map_or(0, |text| text.lines().count())
}

fn serve(world: &World, north: &Repository) -> String {
    let output = world.lander(north, None).wait_with_output().unwrap();
    let text = report(&output);
    assert_eq!(output.status.code(), Some(0), "{text}");
    text
}

fn verdict(north: &Repository, branch: &str) -> String {
    let done = north.queue.join("done");
    fs::read_dir(&done).unwrap().flatten().map(|entry| queue::read(&entry.path()).unwrap()).find(|ticket| ticket.branch == branch).and_then(|ticket| ticket.verdict).unwrap_or_else(|| panic!("{branch} has no verdict in {}", done.display()))
}

fn subjects(north: &Repository) -> Vec<String> {
    git(&north.origin, &["log", "--format=%s", "main"]).lines().map(str::to_owned).collect()
}

#[test]
fn three_queued_lanes_land_in_one_push_with_no_check_of_their_merged_tree_before_it() {
    let world = World::new();
    let north = world.repository("north");
    let calls_file = checker(&world);
    let start = git(&north.origin, &["rev-parse", "main"]);
    for branch in ["a", "b", "c"] {
        north.lane(branch);
    }
    let journal = serve(&world, &north);
    assert_eq!(git(&north.origin, &["rev-list", "--count", &format!("{start}..main")]), "3", "{journal}");
    assert_eq!(journal.matches("lanes that passed their checks before queueing").count(), 1, "{journal}");
    assert!(journal.contains("carries 3 lanes that passed their checks before queueing"), "{journal}");
    assert_eq!(calls(&calls_file), 0, "the lander checked a merged tree before the push: {journal}");
    for branch in ["a", "b", "c"] {
        assert!(verdict(&north, branch).starts_with("landed "), "{journal}");
    }
}

#[test]
fn a_red_after_landing_run_of_a_batch_landed_without_a_merged_tree_check_reverts_exactly_the_culprits() {
    let world = World::new();
    let north = world.repository("north");
    north_base::foundation::shim::write_executable(&world.path().join("bin/north"), AFTER_RUNNER);
    north.lane_with("bad-plan", "origin/main", ("bad.txt", "bad\n"), ("bad-plan (north#360)", None));
    for branch in ["a", "b", "c", "d", "e", "f"] {
        north.lane(branch);
    }
    north.lane_with("bad-context", "origin/main", ("worse.txt", "worse\n"), ("bad-context (north#360)", None));
    let journal = serve(&world, &north);
    assert!(journal.contains("carries 8 lanes that passed their checks before queueing"), "{journal}");
    let subjects = subjects(&north);
    let mut reverts: Vec<String> = subjects.iter().filter(|subject| subject.starts_with("Revert ")).cloned().collect();
    reverts.sort();
    assert_eq!(reverts, ["Revert bad-context (north#360)", "Revert bad-plan (north#360)"], "{subjects:?}\n{journal}");
    let names = git(&north.origin, &["ls-tree", "--name-only", "main"]);
    for file in ["a", "b", "c", "d", "e", "f"] {
        assert!(names.lines().any(|name| name == format!("{file}.txt")), "a good lane was reverted: {file}.txt is missing from {names}\n{journal}");
    }
    assert!(!names.lines().any(|name| name == "bad.txt" || name == "worse.txt"), "{names}\n{journal}");
    let probes = fs::read_to_string(world.path().join("bin/north.probes")).unwrap_or_default();
    let runs = probes.lines().count();
    assert!(runs > 0 && runs <= 6, "two culprits among eight should isolate in at most 2·log2(8) prefix runs, not {runs}: {probes}\n{journal}");
}

const HELD_AFTER_RUNNER: &str = r#"#!/bin/sh
[ "$1" = land ] || exit 0
: > "$HOME/after-started"
while [ ! -e "$HOME/release" ]; do sleep 0.05; done
"#;

fn wait_for(what: &str, ready: impl FnMut() -> bool) {
    assert!(north_base::platform::wait::until(super::serve::HANG, ready), "timed out waiting for {what}");
}

#[test]
fn a_batch_queued_while_the_previous_batch_is_checked_after_landing_is_pushed_before_that_check_ends() {
    let world = World::new();
    let north = world.repository("north");
    north_base::foundation::shim::write_executable(&world.path().join("bin/north"), HELD_AFTER_RUNNER);
    let calls_file = checker(&world);
    north.lane("first");
    let mut lander = world.lander(&north, None);
    let landed = |file: &str| git(&north.origin, &["ls-tree", "--name-only", "main"]).lines().any(|name| name == file);
    wait_for("the first batch's after-landing run to start", || world.path().join("after-started").exists() && landed("first.txt"));
    north.lane("second");
    wait_for("the second batch to be pushed while the first batch's after-landing run is held", || landed("second.txt"));
    assert!(lander.try_wait().unwrap().is_none(), "the lander ended before the held run did");
    assert_eq!(calls(&calls_file), 0, "the second batch waited for a check of a merged tree before its push");
    fs::write(world.path().join("release"), "").unwrap();
    let output = lander.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", report(&output));
}

fn vouch(north: &Repository, branch: &str, base: &str) {
    let path = fs::read_dir(&north.queue).unwrap().flatten().map(|entry| entry.path()).find(|path| path.is_file() && queue::read(path).is_ok_and(|ticket| ticket.branch == branch)).unwrap();
    let ticket = queue::read(&path).unwrap();
    queue::write(&path, &queue::Ticket { alone_green: Some(ticket.head.clone()), alone_base: Some(base.to_owned()), ..ticket }).unwrap();
}

const JUDGE_OUT_ONCE: &str = "#!/bin/sh\nif [ ! -e \"$0.answered\" ]; then\n  : > \"$0.answered\"\n  echo 'the private-text judge could not run (exit 75): no subscription account is available' >&2\n  exit 75\nfi\nexit 0\n";

const NO_SECRETS: &str = "#!/bin/sh\nreport=\nfor arg in \"$@\"; do\n  if [ -n \"$report\" ]; then printf '[]' > \"$arg\"; report=; fi\n  case \"$arg\" in --report-path) report=1 ;; esac\ndone\nexit 0\n";

const PUBLIC_REPO: &str = "#!/bin/sh\necho PUBLIC\n";

/// Queues `lane` through the same `north land enqueue` entry safe-push runs, with a judge that exits 75 once.
fn enqueue(world: &World, lane: &Path) -> Output {
    let bin = world.path().join("bin");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "serve::enqueue_process", "--nocapture", "--test-threads", "1"])
        .env("NORTH_LAND_IT_ARGUMENTS", ["--repo", lane.to_str().unwrap()].join("\n"))
        .env("XDG_STATE_HOME", world.path().join("state"))
        .env("NORTH_TEST_ROOT", world.path())
        .env("HOME", world.path())
        .env("GIT_CONFIG_GLOBAL", world.path().join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
        .env("NORTH_BIN", bin.join("judge"))
        .env("NORTH_LAND_NORTH", bin.join("north"))
        .env("SAFE_PUSH_DENYLIST", world.path().join("denylist.toml"))
        .env("NORTH_PRIVATE_KINDS", world.path().join("private-kinds.toml"))
        .env_remove("SAFE_PUSH_SOUTH")
        .env_remove("AGENT_CAPACITY_NORTH")
        .env_remove("NORTH_FLEET_SESSION")
        .env_remove("NORTH_LAND_SERVING")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.output().unwrap()
}

#[test]
fn a_lane_whose_private_kind_judge_exits_75_is_queued_without_an_alone_pass_and_the_lander_lands_it_once_the_judge_answers() {
    let world = World::new();
    let north = world.repository("north");
    let bin = world.path().join("bin");
    north_base::foundation::shim::write_executable(&bin.join("judge"), JUDGE_OUT_ONCE);
    north_base::foundation::shim::write_executable(&bin.join("betterleaks"), NO_SECRETS);
    north_base::foundation::shim::write_executable(&bin.join("gh"), PUBLIC_REPO);
    fs::write(world.path().join("denylist.toml"), "[[rules]]\nid = \"fixture\"\n").unwrap();
    fs::write(world.path().join("private-kinds.toml"), "[[kind]]\nid = \"address\"\n").unwrap();
    let checker = format!("#!/bin/sh\n{} || exit $?\necho 'safe-push: verdict green'\n", bin.join("judge").display());
    north_base::foundation::shim::write_executable(&bin.join("checker"), &checker);

    let lane = north.checkout.parent().unwrap().join("worktrees").join("prose");
    git(&north.checkout, &["worktree", "add", "-q", "-b", "prose", lane.to_str().unwrap(), "origin/main"]);
    fs::write(lane.join("prose.txt"), "a public note about the method\n").unwrap();
    git(&lane, &["add", "-A"]);
    git(&lane, &["commit", "-q", "-m", "Add a public note"]);

    let output = enqueue(&world, &lane);
    let text = report(&output);
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(text.contains("Done: queued"), "{text}");
    assert!(text.contains("the private-kind judge has no verdict yet"), "{text}");
    assert!(!text.contains("NOT queued"), "{text}");
    let (_, ticket) = queue::pending(&north.queue).unwrap().pop().unwrap_or_else(|| panic!("no ticket in {}", north.queue.display()));
    assert_eq!(ticket.branch, "prose");
    assert_eq!((ticket.alone_green.as_deref(), ticket.alone_base.as_deref()), (None, None), "a lane whose judge had no verdict was vouched for at enqueue");

    let journal = serve(&world, &north);
    assert!(journal.contains("has no alone pass for its head"), "{journal}");
    assert!(journal.contains("passed alone on main@"), "the judge answered, so the lander's alone check did not vouch for the lane: {journal}");
    assert!(verdict(&north, "prose").starts_with("landed "), "{journal}");
    assert!(subjects(&north).iter().any(|subject| subject == "Add a public note"), "{journal}");
}

#[test]
fn a_lane_whose_alone_pass_is_six_landings_old_lands_in_the_same_batch_as_a_fresh_lane_without_another_alone_check() {
    let world = World::new();
    let north = world.repository("north");
    let calls_file = checker(&world);
    let start = git(&north.origin, &["rev-parse", "main"]);
    for landing in 1..=6 {
        north.lane(&format!("l{landing}"));
    }
    serve(&world, &north);
    let main = git(&north.origin, &["rev-parse", "main"]);
    assert_eq!(git(&north.origin, &["rev-list", "--count", &format!("{start}..{main}")]), "6");
    north.lane("stale");
    vouch(&north, "stale", &start);
    north.lane("fresh");
    vouch(&north, "fresh", &main);
    let journal = serve(&world, &north);
    assert!(journal.contains("carries 2 lanes that passed their checks before queueing"), "a lane whose pass aged past a batch was left out of the candidate: {journal}");
    assert!(verdict(&north, "fresh").starts_with("landed "), "{journal}");
    assert!(verdict(&north, "stale").starts_with("landed "), "{journal}");
    assert_eq!(calls(&calls_file), 0, "an aged alone pass held its lane for a serial alone check: {journal}");
}

fn no_verdict_lane_steps_aside_beside_two_landed(checker_output: &str) {
    let world = World::new();
    let north = world.repository("north");
    let path = world.path().join("bin/checker");
    north_base::foundation::shim::write_executable(&path, format!("#!/bin/sh\npwd >> \"$0.calls\"\necho \"{checker_output}\" >&2\nexit 75\n"));
    north.lane("unsettled");
    let ticket = fs::read_dir(&north.queue).unwrap().flatten().map(|entry| entry.path()).find(|path| path.is_file() && queue::read(path).is_ok_and(|ticket| ticket.branch == "unsettled")).unwrap();
    queue::write(&ticket, &queue::Ticket { alone_green: None, alone_base: None, ..queue::read(&ticket).unwrap() }).unwrap();
    north.lane("passed");
    north.lane("also-passed");
    let first = report(&world.lander(&north, None).wait_with_output().unwrap());
    assert!(verdict(&north, "passed").starts_with("landed ") && verdict(&north, "also-passed").starts_with("landed "), "{first}");
    assert!(first.contains("it is checked once more next round"), "{first}");
    let journal = serve(&world, &north);
    let unsettled = verdict(&north, "unsettled");
    assert!(unsettled.starts_with("failed ") && unsettled.contains("reached no verdict twice"), "a lane whose alone check never settles held the queue: {unsettled}\n{journal}");
    assert_eq!(calls(&world.path().join("bin/checker.calls")), 2, "{journal}");
}

#[test]
fn a_lane_whose_alone_check_reaches_no_verdict_twice_steps_aside_while_lanes_with_a_pass_land_beside_it() {
    no_verdict_lane_steps_aside_beside_two_landed("the private-text judge did not finish");
}

#[test]
fn a_lane_whose_solo_check_reaches_no_verdict_never_holds_the_round() {
    no_verdict_lane_steps_aside_beside_two_landed("safe-push: verdict transient main's check environment is broken: the base fails");
}

const HELD_ALONE: &str = "#!/bin/sh\npwd >> \"$0.calls\"\n: > \"$HOME/alone-started\"\nwhile [ ! -e \"$HOME/alone-release\" ]; do sleep 0.05; done\necho 'safe-push: verdict green'\n";

struct Release(std::path::PathBuf);

impl Drop for Release {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, "");
    }
}

#[test]
fn a_candidate_is_pushed_while_an_alone_check_is_in_flight() {
    let world = World::new();
    let north = world.repository("north");
    north_base::foundation::shim::write_executable(&world.path().join("bin/checker"), HELD_ALONE);
    let release = Release(world.path().join("alone-release"));
    let old = runtime_commit(&north, "old");
    let new = runtime_commit(&north, "new");
    std::os::unix::fs::symlink(format!("runtimes/{old}"), world.path().join("lander")).unwrap();
    north.lane("unchecked");
    let (path, ticket) = queue::pending(&north.queue).unwrap().pop().unwrap();
    queue::write(&path, &queue::Ticket { alone_green: None, alone_base: None, ..ticket }).unwrap();
    let (checker, token) = (world.path().join("bin/checker"), world.path().join("bin/token"));
    let mut first = world.land(&["serve", "--repo", north.checkout.to_str().unwrap(), "--checker", checker.to_str().unwrap(), "--app-token", token.to_str().unwrap()]).spawn().unwrap();
    wait_for("the alone check of the lane with no pass to start", || world.path().join("alone-started").exists());
    north.lane("passed");
    let landed = |file: &str| git(&north.origin, &["ls-tree", "--name-only", "main"]).lines().any(|name| name == file);
    let pushed = north_base::platform::wait::until(std::time::Duration::from_secs(60), || landed("passed.txt"));
    fs::remove_file(world.path().join("lander")).unwrap();
    std::os::unix::fs::symlink(format!("runtimes/{new}"), world.path().join("lander")).unwrap();
    let exited = north_base::platform::wait::until(std::time::Duration::from_secs(10), || first.try_wait().unwrap().is_some());
    if !exited {
        let _ = first.kill();
    }
    let handover = report(&first.wait_with_output().unwrap());
    assert!(pushed, "the lane with a pass waited for the alone check in flight instead of landing beside it:\n{handover}");
    assert!(exited, "{handover}");
    let second = world.lander(&north, None);
    drop(release);
    let journal = report(&second.wait_with_output().unwrap());
    assert!(journal.contains("adopting the alone check of unchecked"), "the restarted lander discarded the alone check in flight: {journal}");
    assert!(verdict(&north, "unchecked").starts_with("landed "), "{journal}");
    assert_eq!(calls(&world.path().join("bin/checker.calls")), 1, "the restarted lander ran the alone check again: {journal}");
}
