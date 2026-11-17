use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Output, Stdio};

use landlock::{AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus, path_beneath_rules};
use north_foundation::foundation::shim::write_executable;
use north_base::platform::wait;
use north_lander::land::{enqueue, hand_in, queue};

use super::land_first::{NO_SECRETS, PUBLIC_REPO, queueing};
use super::serve::{HANG, Repository, World, git, report};

const VERB: &str = "hand_in::verb_process";
const VERB_ARGUMENTS: &str = "NORTH_HAND_IN_IT_ARGUMENTS";
const VERB_CONFINED: &str = "NORTH_HAND_IN_IT_CONFINED";
const JUDGE_PASSES: &str = "#!/bin/sh\ncat > /dev/null\nexit 0\n";
const VERDICT_NORTH: &str = r#"#!/bin/sh
cache="$XDG_STATE_HOME/north/verdict-cache"
case "$1 $2 $3 $4" in
  'capacity deadline '*) shift 5; exec "$@" ;;
  'depbox check -- '*) shift 3; exec "$@" ;;
  '--worker-sandbox --program -- '*) shift 3; exec "$@" ;;
  'land graph-build '*) exit 0 ;;
  'capacity offload verdict hit') shift 4; for key; do [ -f "$cache/$key" ] && printf '%s local %s\n' "$key" "$(cat "$cache/$key")"; done; exit 0 ;;
  'capacity offload verdict record') shift 4; while [ $# -gt 0 ]; do case "$1" in --key) key=$2; shift ;; --pass) pass=$2; shift ;; esac; shift; done; [ -z "$pass" ] || { mkdir -p "$cache" && echo "$(date +%s) $pass" > "$cache/$key"; }; exit 0 ;;
esac
exit 1
"#;
const JUDGE_ASKS_A_MODEL_ONCE_A_TEXT: &str = "#!/bin/sh\ntext=$(cat)\nkey=$(printf %s \"$text\" | sha256sum | cut -c1-64)\nif [ ! -e \"$0.verdicts/$key\" ]; then\n  mkdir -p \"$0.verdicts\" && touch \"$0.verdicts/$key\"\nfi\nprintf '%s\\n' \"$text\" | tr '\\n' ' ' >> \"$HOME/judge-answered\"\necho >> \"$HOME/judge-answered\"\nexit 0\n";

fn hold_script(world: &World) -> String {
    let root = world.path().display();
    format!("#!/bin/sh\nneedle=$(sed -n 1p {root}/judge-hold 2>/dev/null)\nwant=$(sed -n 2p {root}/judge-hold 2>/dev/null)\n[ -n \"$want\" ] || exit 0\nwhile :; do\n  have=$(grep -cF -- \"$needle\" {root}/judge-answered 2>/dev/null)\n  [ \"${{have:-0}}\" -lt \"$want\" ] || break\n  sleep 0.05\ndone\n")
}

fn answered(world: &World, note: &str) -> usize {
    fs::read_to_string(world.path().join("judge-answered")).map_or(0, |text| text.lines().filter(|line| line.contains(note)).count())
}

fn hold_check_for(world: &World, note: &str, judges: usize) {
    fs::write(world.path().join("judge-hold"), format!("{note}\n{}\n", answered(world, note) + judges)).unwrap();
}

fn scanned_world(judge: &str) -> (World, Repository) {
    let world = World::new();
    let north = world.repository("north");
    let bin = world.path().join("bin");
    write_executable(&bin.join("judge"), judge);
    write_executable(&bin.join("betterleaks"), NO_SECRETS);
    write_executable(&bin.join("gh"), PUBLIC_REPO);
    fs::write(world.path().join("denylist.toml"), "[[rules]]\nid = \"fixture\"\n").unwrap();
    fs::write(world.path().join("private-kinds.toml"), "[[kind]]\nid = \"address\"\n").unwrap();
    (world, north)
}

fn worktree(north: &Repository, branch: &str) -> PathBuf {
    let lane = north.checkout.parent().unwrap().join("worktrees").join(branch);
    git(&north.checkout, &["worktree", "add", "-q", "-b", branch, lane.to_str().unwrap(), "origin/main"]);
    lane
}

fn verb(world: &World, message: &str, paths: &[&Path]) -> Child {
    let arguments: Vec<&str> = ["--message", message, "--"].into_iter().chain(paths.iter().map(|path| path.to_str().unwrap())).collect();
    queueing(world, VERB, VERB_ARGUMENTS, &arguments).spawn().unwrap()
}

/// Denies this process, and what it starts, removal beneath `denied` and leaves it everywhere else, as a worker sandbox's domain does in a lane its command did not name.
fn confine_removal_beneath(denied: &Path) {
    let removal = AccessFs::RemoveDir | AccessFs::RemoveFile | AccessFs::Refer;
    let elsewhere: Vec<PathBuf> = denied.ancestors().skip(1).filter_map(|parent| fs::read_dir(parent).ok()).flatten().flatten().map(|entry| entry.path()).filter(|path| path.is_dir() && !denied.starts_with(path)).collect();
    let ruleset = Ruleset::default().set_compatibility(CompatLevel::HardRequirement).handle_access(removal).unwrap().create().unwrap();
    let ruleset = ruleset.add_rules(path_beneath_rules(["/"], AccessFs::Refer)).unwrap().add_rules(path_beneath_rules(elsewhere, removal)).unwrap();
    assert!(matches!(ruleset.restrict_self().unwrap().ruleset, RulesetStatus::FullyEnforced), "this host cannot deny removal in a lane");
}

fn lander(world: &World, north: &Repository) -> Output {
    let bin = world.path().join("bin");
    let mut lander = world.land(&["serve", "--once", "--repo", north.checkout.to_str().unwrap(), "--checker", bin.join("checker").to_str().unwrap(), "--app-token", bin.join("token").to_str().unwrap()]);
    lander
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
        .env("NORTH_BIN", bin.join("judge"))
        .env("SAFE_PUSH_DENYLIST", world.path().join("denylist.toml"))
        .env("NORTH_PRIVATE_KINDS", world.path().join("private-kinds.toml"))
        .env_remove("SAFE_PUSH_SOUTH");
    lander.output().unwrap()
}

/// Waits for `hand` to queue its lane, serves the queue once and returns what `hand` printed once the lander decided.
fn decided(world: &World, north: &Repository, mut hand: Child) -> (Output, String) {
    let queued = || queue::pending(&north.queue).is_ok_and(|pending| !pending.is_empty());
    assert!(wait::until(HANG, || queued() || hand.try_wait().unwrap().is_some()), "the hand-in neither queued nor exited");
    let journal = if queued() { report(&lander(world, north)) } else { String::new() };
    (hand.wait_with_output().unwrap(), journal)
}

fn refusals(output: &Output) -> Vec<String> {
    [&output.stdout, &output.stderr].into_iter().flat_map(|stream| String::from_utf8_lossy(stream).lines().filter(|line| line.contains('✗')).map(str::to_owned).collect::<Vec<_>>()).collect()
}

#[test]
fn verb_process() {
    let Ok(arguments) = std::env::var(VERB_ARGUMENTS) else { return };
    let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
    if let Ok(denied) = std::env::var(VERB_CONFINED) {
        confine_removal_beneath(Path::new(&denied));
    }
    let road = |lane: &hand_in::Lane| enqueue::run(&["--repo".to_owned(), lane.root.display().to_string(), "--wait".to_owned()]);
    std::process::exit(i32::from(hand_in::run(&arguments, road)));
}

const FINISH: &str = "hand_in::a_finish_call_queues_only_after_its_check_passes_and_returns_the_ticket_before_the_verdict_and_each_hand_in_fact_is_fetched_once";
const FINISH_ARGUMENTS: &str = "NORTH_FINISH_IT_ARGUMENTS";

fn finishing(world: &World, session: &str, check: &str, message: &str, path: &Path) -> std::process::Command {
    let arguments = ["--check", check, "--message", message, "--", path.to_str().unwrap()];
    let mut command = queueing(world, FINISH, FINISH_ARGUMENTS, &arguments);
    command.env("NORTH_FLEET_SESSION", session).env("NORTH_VERDICT_CACHE_DIR", world.path().join("verdicts"));
    command
}

fn finish(world: &World, session: &str, check: &str, message: &str, path: &Path) -> Output {
    finishing(world, session, check, message, path).output().unwrap()
}

fn counting_git(real: &Path) -> String {
    format!("#!/bin/sh\nfor argument; do\n  case \"$argument\" in fetch | *public-denylist.toml) printf '%s\\n' \"$*\" >> \"$0.calls\"; break;; esac\ndone\nexec {} \"$@\"\n", real.display())
}

const COUNTING_GH: &str = "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\ncase \"$*\" in *isArchived*) echo false PUBLIC;; *) echo PUBLIC;; esac\n";

fn south_with_a_denylist(world: &World) -> PathBuf {
    let (south, main, origin) = (world.path().join("south"), world.path().join("south/main"), world.path().join("south-origin.git"));
    fs::create_dir_all(main.join("privacy")).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    fs::write(main.join("privacy/public-denylist.toml"), "[[rules]]\nid = \"fixture\"\n").unwrap();
    git(&main, &["add", "privacy"]);
    git(&main, &["commit", "-qm", "Add the deny-list"]);
    git(world.path(), &["clone", "-q", "--bare", main.to_str().unwrap(), origin.to_str().unwrap()]);
    git(&main, &["remote", "add", "origin", origin.to_str().unwrap()]);
    south
}

fn calls(world: &World, program: &str) -> Vec<String> {
    fs::read_to_string(world.path().join("bin").join(format!("{program}.calls"))).unwrap_or_default().lines().map(str::to_owned).collect()
}

#[test]
fn a_finish_call_queues_only_after_its_check_passes_and_returns_the_ticket_before_the_verdict_and_each_hand_in_fact_is_fetched_once() {
    if let Ok(arguments) = std::env::var(FINISH_ARGUMENTS) {
        let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
        let road = |lane: &hand_in::Lane| enqueue::run(&["--repo".to_owned(), lane.root.display().to_string()]);
        std::process::exit(i32::from(hand_in::finish(&arguments, road)));
    }
    let (world, north) = scanned_world(JUDGE_ASKS_A_MODEL_ONCE_A_TEXT);
    write_executable(&world.path().join("bin/north"), VERDICT_NORTH);
    write_executable(&world.path().join("bin/hold-judges"), hold_script(&world));
    let held = format!("test ! -e red.txt && {}", world.path().join("bin/hold-judges").display());
    fs::write(north.checkout.join(".safe-push"), format!("[landing]\n\tpush-app = lander\n[land]\n\tcheck = ** :: {held}\n")).unwrap();
    git(&north.checkout, &["commit", "-qam", "Run a land check before queueing"]);
    git(&north.checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    git(&north.checkout, &["fetch", "-q", "origin"]);
    let lane = worktree(&north, "finish");
    fs::write(lane.join("named.txt"), "the finished change\n").unwrap();
    fs::write(lane.join("unnamed.txt"), "a change the worker did not name\n").unwrap();
    let start = git(&lane, &["rev-parse", "HEAD"]);

    let placed = north_lander::land::freeze::Freeze { cause: "the history cut rewrites main".to_owned(), placed_at: 0, placed_on: "laptop".to_owned() };
    let freeze = north_lander::land::freeze::path(&world.path().join("state"));
    fs::create_dir_all(freeze.parent().unwrap()).unwrap();
    fs::write(&freeze, serde_json::to_string(&placed).unwrap()).unwrap();
    let frozen_ran = world.path().join("frozen-check-ran");
    let frozen = finish(&world, "frozen", &format!("touch {}", frozen_ran.display()), "Add a note during the cut (north#1281)", &lane.join("named.txt"));
    let text = report(&frozen);
    assert_eq!(frozen.status.code(), Some(1), "{text}");
    assert!(refusals(&frozen).iter().any(|line| line.contains("the landing is frozen: the history cut rewrites main") && line.contains("NOT queued")), "a finish call during a freeze was not refused naming the freeze and its cause (north#1281): {text}");
    assert!(!frozen_ran.exists(), "the check ran during a freeze: {text}");
    assert_eq!(git(&lane, &["rev-parse", "HEAD"]), start, "a frozen finish call committed: {text}");
    assert!(queue::pending(&north.queue).unwrap_or_default().is_empty(), "a frozen finish call queued a ticket: {text}");
    fs::remove_file(&freeze).unwrap();

    let failed = finish(&world, "proposer", "echo the box is red; exit 3", "Add the finished note (north#1194)", &lane.join("named.txt"));
    let text = report(&failed);
    assert_eq!(failed.status.code(), Some(1), "{text}");
    assert!(text.contains("the box is red") && text.contains("exited 3") && text.contains("NOT queued"), "a failed check did not return its output: {text}");
    assert_eq!(git(&lane, &["rev-parse", "HEAD"]), start, "a failed check committed: {text}");
    assert_eq!(git(&lane, &["diff", "--cached", "--name-only"]), "", "a failed check staged: {text}");
    assert!(queue::pending(&north.queue).unwrap().is_empty(), "a failed check queued: {text}");
    let kept = text.lines().find_map(|line| line.split("the attempt is kept in ").nth(1)).unwrap_or_else(|| panic!("a failed check named no kept attempt: {text}"));
    let attempt: serde_json::Value = serde_json::from_str(&fs::read_to_string(kept.trim()).unwrap()).unwrap();
    assert_eq!(attempt["output"], "the box is red\n", "the kept attempt lost the check's output: {attempt}");
    assert!(attempt["diff"].as_str().is_some_and(|diff| diff.contains("+the finished change") && !diff.contains("unnamed")), "the kept attempt does not hold the named change as its diff: {attempt}");
    let store = north_base::platform::finish_record::FinishRecords(world.path().join("verdicts").join("finished"));
    let recorded = store.read("proposer").unwrap_or_else(|| panic!("a failed finish call wrote no finish record outside the lane: {text}"));
    assert!(!recorded.passed && recorded.command == "echo the box is red; exit 3", "the finish record does not name the failed acceptance command: {recorded:?}");
    let made = worktree(&north, "made");
    let listed = world.path().join("files.json");
    fs::write(&listed, serde_json::json!([{"path": made.join("made.txt"), "content": "the change a session made in its own lane\n"}]).to_string()).unwrap();
    let from_elsewhere = queueing(&world, FINISH, FINISH_ARGUMENTS, &["--check", "echo the made lane is red; exit 3", "--message", "Add the made note (north#1602)", "--files", listed.to_str().unwrap()]).env("NORTH_FLEET_SESSION", "maker").env("NORTH_VERDICT_CACHE_DIR", world.path().join("verdicts")).output().unwrap();
    let text = report(&from_elsewhere);
    assert!(text.contains("the made lane is red") && text.contains("NOT queued") && !text.contains("is outside the lane"), "a finish call with files in the lane the session made was bound to the directory it ran in (north#1602): {text}");
    assert_eq!(fs::read_to_string(made.join("made.txt")).unwrap(), "the change a session made in its own lane\n", "{text}");
    let narrow = worktree(&north, "narrow");
    fs::write(narrow.join("red.txt"), "a change the named check does not read\n").unwrap();
    let refused = finishing(&world, "small", "true", "Add the red note (north#1233)", &narrow.join("red.txt")).output().unwrap();
    let text = report(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(queue::pending(&north.queue).unwrap().is_empty(), "a lane one of the repository's checks before queueing failed was queued: {text}");
    assert!(store.read("small").is_some_and(|recorded| !recorded.passed && recorded.command == "true"), "a finish call whose named check passed left a passing finish record for a lane the repository's check before queueing failed, so a route's first rung never moves to its second (north#1233): {text}");
    let refused_attempt = text.lines().find_map(|line| line.split("the attempt is kept in ").nth(1)).unwrap_or_else(|| panic!("a lane refused by a check before queueing named no kept attempt: {text}"));
    let attempt: serde_json::Value = serde_json::from_str(&fs::read_to_string(refused_attempt.trim()).unwrap()).unwrap();
    assert!(attempt["output"].as_str().is_some_and(|output| output.contains("test ! -e red.txt")) && attempt["diff"].as_str().is_some_and(|diff| diff.contains("+a change the named check does not read")), "the kept attempt names neither the repository's failed check nor the refused change: {attempt}");

    let mut proposing = queueing(&world, FINISH, FINISH_ARGUMENTS, &["--judge-message", "proposer"]).current_dir(&lane).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    proposing.stdin.take().unwrap().write_all(b"Add the proposed note (north#1239)").unwrap();
    let proposed = proposing.wait_with_output().unwrap();
    assert!(String::from_utf8_lossy(&proposed.stdout).split_whitespace().any(|word| word == hand_in::PROPOSED), "the launch-time scan did not pass the proposed message on to its judge: {}", report(&proposed));
    let stale = finish(&world, "later", "true", "", &lane.join("named.txt"));
    assert_eq!(stale.status.code(), Some(1), "another session's finish call committed the proposal kept for an earlier one: {}", report(&stale));
    assert!(queue::pending(&north.queue).unwrap().is_empty(), "another session's empty-message finish call queued: {}", report(&stale));
    fs::write(lane.join("named.txt"), "the finished change, checked\n").unwrap();
    let unproved = finishing(&world, "proposer", "echo the unproved check ran; exit 3", "", &lane.join("named.txt")).output().unwrap();
    let unproved_text = report(&unproved);
    assert!(unproved_text.contains("the unproved check ran") && !unproved_text.contains("the brief proposed none"), "a finish process no broker proved did not read the proposal kept for its session: {unproved_text}");
    let ran = world.path().join("check-ran");
    hold_check_for(&world, "Add the proposed note", 2);
    let passed = finish(&world, "proposer", &format!("test -e named.txt && touch {}", ran.display()), "", &lane.join("named.txt"));
    fs::remove_file(world.path().join("judge-hold")).unwrap();
    let text = report(&passed);
    assert_eq!(passed.status.code(), Some(0), "{text}");
    assert!(ran.exists(), "the check did not run in the lane: {text}");
    assert!(!Path::new(kept.trim()).exists(), "a passing check left the failed attempt for a second rung to read: {text}");
    assert!(store.read("proposer").is_some_and(|recorded| recorded.passed), "a passing finish call left its session's failed finish record");
    let pending = queue::pending(&north.queue).unwrap();
    let [(ticket, queued)] = &pending[..] else { panic!("the passing call did not leave one undecided ticket: {pending:?}\n{text}") };
    let name = ticket.file_name().unwrap().to_str().unwrap();
    assert!(text.lines().any(|line| line.starts_with("Done: queued") && line.ends_with(&format!("ticket {name}"))), "the call did not return its ticket: {text}");
    assert_eq!(git(&lane, &["diff-tree", "--no-commit-id", "--name-only", "-r", &queued.head]), "named.txt", "the call committed more than the named path");
    assert_eq!(git(&lane, &["log", "-1", "--format=%B", &queued.head]).trim_end(), "Add the proposed note (north#1239)", "a finish call that left out its message did not commit the proposed one byte for byte");
    assert_eq!(git(&lane, &["status", "--porcelain", "--", "unnamed.txt"]), "?? unnamed.txt", "the call swept up a path it was not given");
    assert_eq!((queued.alone_green.as_deref(), queued.unjudged_on.as_deref()), (Some(queued.head.as_str()), None), "the judge answered while the checks before queueing ran, yet the ticket waits for the lander to ask again (north#1239): {text}");

    let real_git = std::env::split_paths(&std::env::var_os("PATH").unwrap()).map(|dir| dir.join("git")).find(|git| git.is_file()).unwrap();
    write_executable(&world.path().join("bin/git"), counting_git(&real_git));
    write_executable(&world.path().join("bin/gh"), COUNTING_GH);
    let south = south_with_a_denylist(&world);
    let counted = worktree(&north, "facts");
    fs::write(counted.join("facts.txt"), "a change whose facts are counted\n").unwrap();
    hold_check_for(&world, "Add the counted note", 1);
    let mut counting = finishing(&world, "facts", &held, "Add the counted note (north#1239)", &counted.join("facts.txt"));
    let counted_text = report(&counting.env_remove("SAFE_PUSH_DENYLIST").env("SAFE_PUSH_SOUTH", &south).output().unwrap());
    assert!(counted_text.lines().any(|line| line.starts_with("Done: queued")), "{counted_text}");
    fs::remove_file(world.path().join("judge-hold")).unwrap();
    let (fetches, lookups) = (calls(&world, "git"), calls(&world, "gh"));
    let count = |calls: &[String], kind: &str| calls.iter().filter(|call| call.contains(kind)).count();
    let tally = (count(&fetches, "refs/remotes/origin/main"), count(&fetches, "refs/south/main"), count(&fetches, "public-denylist.toml"), fetches.len(), count(&lookups, "repo view"));
    assert_eq!(tally, (1, 2, 2, 5, 1), "a hand-in fetched the destination once, the deny-list copy twice (the early ask's copy and the enqueue gate's own, taken after the gate began, 7af1c218f50) and looked the repository up once; got:\n{fetches:#?}\n{lookups:#?}");
}

#[test]
fn a_finish_call_refuses_while_another_repositorys_lane_of_the_same_name_holds_an_unlanded_commit() {
    if let Ok(arguments) = std::env::var(FINISH_ARGUMENTS) {
        let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
        let road = |lane: &hand_in::Lane| enqueue::run(&["--repo".to_owned(), lane.root.display().to_string()]);
        std::process::exit(i32::from(hand_in::finish(&arguments, road)));
    }
    let (world, north) = scanned_world(JUDGE_PASSES);
    let wisp = world.repository("wisp");
    let lane = worktree(&north, "worker");
    let other = wisp.checkout.parent().unwrap().join("worktrees/worker");
    git(&wisp.checkout, &["worktree", "add", "-q", "-b", "worker", other.to_str().unwrap(), "origin/main"]);
    fs::write(other.join("second.txt"), "the second repository's half\n").unwrap();
    git(&other, &["add", "second.txt"]);
    git(&other, &["commit", "-qm", "Commit the second repository's half"]);
    fs::write(lane.join("named.txt"), "the first repository's half\n").unwrap();

    let refused = finish(&world, "two", "true", "Add the first half (north#1233)", &lane.join("named.txt"));
    let text = report(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(text.contains(&other.display().to_string()) && text.contains("NOT queued"), "the refusal does not name the other repository's lane: {text}");
    assert!(queue::pending(&north.queue).unwrap().is_empty(), "a finish call queued with the other repository's commit unlanded: {text}");

    git(&other, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    git(&other, &["fetch", "-q", "origin"]);
    let passed = finish(&world, "two", "true", "Add the first half (north#1233)", &lane.join("named.txt"));
    assert_eq!(passed.status.code(), Some(0), "{}", report(&passed));
    assert_eq!(queue::pending(&north.queue).unwrap().len(), 1, "the call did not queue once the other repository's commit was on its main: {}", report(&passed));
}


#[test]
fn a_hand_in_that_names_a_deleted_path_lands_its_deletion() {
    if let Ok(arguments) = std::env::var(VERB_ARGUMENTS) {
        let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
        let road = |lane: &hand_in::Lane| enqueue::run(&["--repo".to_owned(), lane.root.display().to_string(), "--wait".to_owned()]);
        std::process::exit(i32::from(hand_in::run(&arguments, road)));
    }
    let (world, north) = scanned_world(JUDGE_PASSES);
    fs::create_dir_all(north.checkout.join("old/inner")).unwrap();
    for file in ["old/a.txt", "old/inner/b.txt", "kept.txt", "gone.txt"] {
        fs::write(north.checkout.join(file), "to be removed\n").unwrap();
    }
    git(&north.checkout, &["add", "old", "kept.txt", "gone.txt"]);
    git(&north.checkout, &["commit", "-qm", "Add files to remove"]);
    git(&north.checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let lane = worktree(&north, "removal");
    fs::remove_file(lane.join("README")).unwrap();
    fs::remove_dir_all(lane.join("old")).unwrap();
    fs::remove_file(lane.join("kept.txt")).unwrap();
    git(&lane, &["rm", "-q", "gone.txt"]);
    git(&lane, &["commit", "-qm", "Delete gone.txt"]);
    let start = git(&lane, &["rev-parse", "HEAD"]);

    let refused = verb(&world, "Remove what never existed", &[&lane.join("never/existed.txt")]).wait_with_output().unwrap();
    let text = report(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(text.contains("never/existed.txt") && text.contains("not in the lane") && text.contains("NOT committed"), "{text}");
    assert_eq!(git(&lane, &["rev-parse", "HEAD"]), start, "a refused hand-in committed: {text}");

    let (output, journal) = decided(&world, &north, verb(&world, "Remove the old files (north#1268)", &[&lane.join("README"), &lane.join("old"), &lane.join("gone.txt")]));
    let text = report(&output);
    let landed = git(&north.origin, &["rev-parse", "main"]);
    assert_eq!(output.status.code(), Some(0), "{text}\n{journal}");
    assert!(text.contains(&format!("Done: landed {landed} on main")), "{text}\n{journal}");
    assert_eq!(git(&north.origin, &["diff-tree", "--no-commit-id", "--name-status", "-r", "main"]), "D\tREADME\nD\tgone.txt\nD\told/a.txt\nD\told/inner/b.txt", "the deletions did not land, or more than the named paths did");
    assert_eq!((git(&lane, &["diff", "--cached", "--name-only"]), git(&lane, &["ls-files", "--deleted"])), (String::new(), "kept.txt".to_owned()), "an unnamed deletion was staged");

    git(&north.checkout, &["fetch", "-q", "origin"]);
    let lane = worktree(&north, "literal");
    fs::write(lane.join("*.txt"), "the starred file\n").unwrap();
    fs::write(lane.join("a.txt"), "the plain file\n").unwrap();
    let start = git(&lane, &["rev-parse", "HEAD"]);

    for magic in [":/", ":(glob)**"] {
        let refused = verb(&world, "Stage everything", &[&lane.join(magic)]).wait_with_output().unwrap();
        let text = report(&refused);
        assert_eq!(refused.status.code(), Some(1), "{text}");
        assert!(text.contains(&format!("{}", lane.join(magic.trim_end_matches('/')).display())) && text.contains("not in the lane") && text.contains("NOT committed"), "{text}");
        assert_eq!(git(&lane, &["rev-parse", "HEAD"]), start, "a refused hand-in committed: {text}");
    }

    let (output, journal) = decided(&world, &north, verb(&world, "Land the starred file (north#1268)", &[&lane.join("*.txt")]));
    let text = report(&output);
    let landed = git(&north.origin, &["rev-parse", "main"]);
    assert_eq!(output.status.code(), Some(0), "{text}\n{journal}");
    assert!(text.contains(&format!("Done: landed {landed} on main")), "{text}\n{journal}");
    assert_eq!(git(&north.origin, &["diff-tree", "--no-commit-id", "--name-status", "-r", "main"]), "A\t*.txt", "a glob in a name staged more than that file");
    assert_eq!((git(&lane, &["diff", "--cached", "--name-only"]), git(&lane, &["ls-files", "--others"])), (String::new(), "a.txt".to_owned()), "the unnamed file was staged");
}

#[test]
fn a_hand_in_that_deletes_a_tracked_file_lands_its_deletion() {
    let (world, north) = scanned_world(JUDGE_PASSES);
    let lane = worktree(&north, "staged-removal");
    git(&lane, &["rm", "-q", "README"]);

    let (output, journal) = decided(&world, &north, verb(&world, "Remove the readme (north#1379)", &[&lane.join("README")]));
    let text = report(&output);
    let landed = git(&north.origin, &["rev-parse", "main"]);
    assert_eq!(output.status.code(), Some(0), "{text}\n{journal}");
    assert!(text.contains(&format!("Done: landed {landed} on main")), "{text}\n{journal}");
    assert_eq!(git(&north.origin, &["diff-tree", "--no-commit-id", "--name-status", "-r", "main"]), "D\tREADME", "{text}");
}

fn handing(world: &World, message: &str, path: &Path) -> Child {
    queueing(world, "hand_in::a_handed_in_lane_keeps_its_message_and_live_fix_trailer", VERB_ARGUMENTS, &["--message", message, "--", path.to_str().unwrap()]).spawn().unwrap()
}

#[test]
fn a_handed_in_lane_keeps_its_message_and_live_fix_trailer() {
    if let Ok(arguments) = std::env::var(VERB_ARGUMENTS) {
        let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
        match std::env::var("NORTH_HAND_IN_IT_CHECK_ONLY").ok().as_deref() {
            Some("check-only") => {
                let cwd = std::env::current_dir().unwrap();
                std::process::exit(i32::from(enqueue::run(&["--repo".to_owned(), cwd.display().to_string(), "--check-only".to_owned()])));
            }
            Some("hand-in") => {
                let state = Path::new(&std::env::var("NORTH_TEST_ROOT").unwrap()).join("state");
                std::process::exit(i32::from(hand_in::run(&arguments, CheckOnlyRoad { state })));
            }
            _ => {}
        }
        let road = |lane: &hand_in::Lane| {
            let file = Path::new(&std::env::var("NORTH_TEST_ROOT").unwrap()).join("landing-message");
            fs::write(&file, &lane.message).unwrap();
            enqueue::run(&["--repo".to_owned(), lane.root.display().to_string(), "--message".to_owned(), file.display().to_string(), "--wait".to_owned()])
        };
        std::process::exit(i32::from(hand_in::run(&arguments, road)));
    }
    let (world, north) = scanned_world(JUDGE_PASSES);
    let lane = worktree(&north, "carried");
    fs::write(lane.join("carried.txt"), "the committed change\n").unwrap();
    git(&lane, &["add", "carried.txt"]);
    let message = "Name the change once\n\nWhy it lands.\n\nLive-fix: the lander is red";
    git(&lane, &["commit", "-qm", message]);

    let (output, journal) = decided(&world, &north, handing(&world, "", &lane.join("carried.txt")));
    let text = report(&output);
    assert_eq!(output.status.code(), Some(0), "{text}\n{journal}");
    assert_eq!(git(&north.origin, &["log", "-1", "--format=%B", "main"]), message, "a committed lane handed in with no message did not land under its head's message: {text}");

    git(&north.checkout, &["fetch", "-q", "origin"]);
    let lane = worktree(&north, "named");
    fs::write(lane.join("named.txt"), "the named change\n").unwrap();
    git(&lane, &["add", "named.txt"]);
    git(&lane, &["commit", "-qm", "Name the change twice\n\nWhy it lands.\n\nItem: north#1540\nBox: the lane head's box\nLive-fix: the lander is red\n\nCo-Authored-By: A <a@example.com>"]);
    let (output, journal) = decided(&world, &north, handing(&world, "Name the change twice", &lane.join("named.txt")));
    let text = report(&output);
    assert_eq!(output.status.code(), Some(0), "{text}\n{journal}");
    assert_eq!(
        git(&north.origin, &["log", "-1", "--format=%B", "main"]),
        "Name the change twice\n\nWhy it lands.\n\nItem: north#1540\nBox: the lane head's box\nLive-fix: the lander is red\nCo-Authored-By: A <a@example.com>",
        "a hand-in message of its subject alone dropped the lane head's body or trailers, as 5985b7d80e lost its body: {text}"
    );
    hand_in_requiring_check_only_refuses_uncommitted_or_unchecked_heads_and_queues_a_checked_head(&world, &north);
}

struct CheckOnlyRoad {
    state: PathBuf,
}

impl hand_in::Road for CheckOnlyRoad {
    fn requires_check_only(&self) -> bool {
        true
    }

    fn admit(&self, admit: &hand_in::Admit) -> Result<(), String> {
        if admit.uncommitted {
            return Err(format!("the lane has uncommitted changes and has not passed safe-push --check-only at its current head ({}); commit the changes and run safe-push --check-only first", admit.head));
        }
        let requested = admit.requested_message.trim();
        if !requested.is_empty() {
            let full = git(&admit.lane.root, &["show", "-s", "--format=%B", "HEAD"]);
            let subject = full.lines().next().unwrap_or_default().trim();
            if requested != full.trim() && requested != subject {
                return Err(format!("--message differs from HEAD ({}); amend the commit and run safe-push --check-only first", admit.head));
            }
        }
        let origin = git(&admit.lane.root, &["remote", "get-url", "--push", "origin"]);
        let dir = north_lander::land::check_only::store_dir(&self.state);
        if !north_lander::land::check_only::passed(&dir, &origin, &admit.lane.destination, admit.head) {
            return Err(format!("the lane has not passed safe-push --check-only at its current head ({}); run safe-push --check-only first", admit.head));
        }
        Ok(())
    }

    fn take(self, lane: &hand_in::Lane) -> u8 {
        enqueue::run(&["--repo".to_owned(), lane.root.display().to_string(), "--require-check-only".to_owned(), "--wait".to_owned()])
    }
}

fn hand_in_requiring_check_only_refuses_uncommitted_or_unchecked_heads_and_queues_a_checked_head(world: &World, north: &Repository) {
    const CHECK_ONLY_MODE: &str = "NORTH_HAND_IN_IT_CHECK_ONLY";
    git(&north.checkout, &["fetch", "-q", "origin"]);
    let lane = worktree(north, "checked-head");
    let target = lane.join("checked.txt");
    fs::write(&target, "uncommitted\n").unwrap();
    let call = |mode: &str, message: &str| {
        queueing(world, "hand_in::a_handed_in_lane_keeps_its_message_and_live_fix_trailer", VERB_ARGUMENTS, &["--message", message, "--", target.to_str().unwrap()])
            .current_dir(&lane)
            .env(CHECK_ONLY_MODE, mode)
            .env("NORTH_VERDICT_CACHE_DIR", world.path().join("verdicts"))
            .spawn()
            .unwrap()
    };
    let dirty = call("hand-in", "Uncommitted change").wait_with_output().unwrap();
    assert_eq!(dirty.status.code(), Some(1), "{}", report(&dirty));
    assert!(report(&dirty).contains("safe-push --check-only"), "{}", report(&dirty));

    git(&lane, &["add", "checked.txt"]);
    git(&lane, &["commit", "-qm", "Checked change (north#1650)\n\nBody.\n\nItem: north#1650\n\nCo-Authored-By: A <a@example.com>"]);
    let checked_sha = git(&lane, &["rev-parse", "HEAD"]);
    let unchecked = call("hand-in", "Checked change (north#1650)").wait_with_output().unwrap();
    assert_eq!(unchecked.status.code(), Some(1), "{}", report(&unchecked));
    assert!(report(&unchecked).contains("safe-push --check-only"), "{}", report(&unchecked));

    let checked_run = call("check-only", "").wait_with_output().unwrap();
    assert_eq!(checked_run.status.code(), Some(0), "{}", report(&checked_run));
    assert!(report(&checked_run).contains("Done: safe-push --check-only passed"), "{}", report(&checked_run));
    assert!(queue::pending(&north.queue).unwrap().is_empty(), "check-only queued a ticket");

    let edited_message = call("hand-in", "Rewritten subject (north#1650)").wait_with_output().unwrap();
    assert_eq!(edited_message.status.code(), Some(1), "{}", report(&edited_message));
    assert_eq!(git(&lane, &["rev-parse", "HEAD"]), checked_sha, "a refused message edit mutated HEAD");

    fs::write(lane.join("README"), "dirty tracked sibling\n").unwrap();
    let dirty_sibling = call("hand-in", "Checked change (north#1650)").wait_with_output().unwrap();
    assert_eq!(dirty_sibling.status.code(), Some(1), "{}", report(&dirty_sibling));
    git(&lane, &["checkout", "--", "README"]);

    let (landed_out, journal) = decided(world, north, call("hand-in", "Checked change (north#1650)"));
    assert_eq!(landed_out.status.code(), Some(0), "{}\n{journal}", report(&landed_out));
    assert_eq!(git(&lane, &["rev-parse", "HEAD"]), checked_sha);
    assert_eq!(git(&north.origin, &["log", "-1", "--format=%B", "main"]).trim(), "Checked change (north#1650)\n\nBody.\n\nItem: north#1650\n\nCo-Authored-By: A <a@example.com>");

    git(&north.checkout, &["fetch", "-q", "origin"]);
    let moved_lane = worktree(north, "moved-head");
    let moved_target = moved_lane.join("moved.txt");
    fs::write(&moved_target, "v1\n").unwrap();
    git(&moved_lane, &["add", "moved.txt"]);
    git(&moved_lane, &["commit", "-qm", "First head (north#1650)"]);
    let run_moved = |mode: &str, message: &str| {
        queueing(world, "hand_in::a_handed_in_lane_keeps_its_message_and_live_fix_trailer", VERB_ARGUMENTS, &["--message", message, "--", moved_target.to_str().unwrap()])
            .current_dir(&moved_lane)
            .env(CHECK_ONLY_MODE, mode)
            .env("NORTH_VERDICT_CACHE_DIR", world.path().join("verdicts"))
            .output()
            .unwrap()
    };
    assert_eq!(run_moved("check-only", "").status.code(), Some(0));
    fs::write(&moved_target, "v2\n").unwrap();
    git(&moved_lane, &["commit", "-qam", "Second head (north#1650)"]);
    let refused_moved = run_moved("hand-in", "Second head (north#1650)");
    assert_eq!(refused_moved.status.code(), Some(1), "{}", report(&refused_moved));
    assert!(report(&refused_moved).contains("safe-push --check-only"), "{}", report(&refused_moved));
}

#[test]
fn a_hand_in_behind_main_is_rebased_onto_it_before_its_checks_a_stopped_rebase_or_a_lane_it_may_not_remove_in_keeps_no_untracked_file_and_a_real_conflict_stays_a_refusal_naming_the_file() {
    let (world, north) = scanned_world(JUDGE_PASSES);
    fs::create_dir_all(north.checkout.join("zzz")).unwrap();
    fs::write(north.checkout.join("zzz/keep.txt"), "kept\n").unwrap();
    git(&north.checkout, &["add", "-A"]);
    git(&north.checkout, &["commit", "-qm", "Add a directory main keeps"]);
    git(&north.checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let behind = worktree(&north, "behind");
    let conflicting = worktree(&north, "conflicting");
    let stopped = worktree(&north, "stopped");
    let confined = worktree(&north, "confined");
    fs::write(north.checkout.join("moved.txt"), "main moved on\n").unwrap();
    fs::write(north.checkout.join("README"), "main's readme\n").unwrap();
    fs::create_dir_all(north.checkout.join("added")).unwrap();
    fs::write(north.checkout.join("added/new.txt"), "a file main added\n").unwrap();
    fs::write(north.checkout.join("zzz/keep.txt"), "main changed it\n").unwrap();
    git(&north.checkout, &["add", "-A"]);
    git(&north.checkout, &["commit", "-qm", "Move main on"]);
    git(&north.checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let moved = git(&north.origin, &["rev-parse", "main"]);

    fs::write(behind.join("named.txt"), "a change made before main moved\n").unwrap();
    fs::write(behind.join("unnamed.txt"), "a change the worker did not name\n").unwrap();
    let (output, journal) = decided(&world, &north, verb(&world, "Add the note made behind main (north#1239)", &[&behind.join("named.txt")]));
    let text = report(&output);
    assert_eq!(output.status.code(), Some(0), "{text}\n{journal}");
    assert!(text.contains("rebased the lane's 0 commit(s)") && text.contains("onto origin/main"), "the hand-in did not say it rebased the lane: {text}");
    assert_eq!(git(&behind, &["rev-parse", "HEAD~1"]), moved, "the lane was not rebased onto main before it was committed and checked: {text}");
    assert_eq!(fs::read_to_string(behind.join("unnamed.txt")).unwrap(), "a change the worker did not name\n", "the rebase lost an uncommitted change: {text}");

    fs::write(conflicting.join("README"), "the lane's readme\n").unwrap();
    let start = git(&conflicting, &["rev-parse", "HEAD"]);
    let refused = verb(&world, "Rewrite the readme", &[&conflicting.join("README")]).wait_with_output().unwrap();
    let text = report(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(text.contains("conflicts with origin/main") && text.contains("README"), "a real conflict was not refused naming its file: {text}");
    assert_eq!(git(&conflicting, &["rev-parse", "HEAD~1"]), start, "a conflicting lane was moved: {text}");
    assert_eq!(fs::read_to_string(conflicting.join("README")).unwrap(), "the lane's readme\n", "a conflicting lane lost its change: {text}");

    fs::write(stopped.join("stopped.txt"), "a change the stopped rebase must not leave behind\n").unwrap();
    let open = fs::metadata(stopped.join("zzz")).unwrap().permissions();
    fs::set_permissions(stopped.join("zzz"), fs::Permissions::from_mode(0o555)).unwrap();
    let queued = || queue::pending(&north.queue).is_ok_and(|pending| !pending.is_empty());
    let mut stopping = verb(&world, "Add the note the stopped rebase must not leave behind", &[&stopped.join("stopped.txt")]);
    assert!(wait::until(HANG, || queued() || stopping.try_wait().unwrap().is_some()), "the stopped hand-in neither queued nor exited");
    fs::set_permissions(stopped.join("zzz"), open).unwrap();
    let untracked = git(&stopped, &["ls-files", "--others", "--exclude-standard"]);
    let added = stopped.join("added").exists();
    if queued() {
        lander(&world, &north);
    }
    let text = report(&stopping.wait_with_output().unwrap());
    assert!(!text.contains("rebased the lane's"), "a lane whose rebase stopped was reported as rebased: {text}");
    assert_eq!(untracked, "", "a rebase that stopped left files its HEAD does not track: {text}");
    assert!(!added, "a rebase that stopped left main's added directory in the lane: {text}");

    let note = confined.join("confined.txt");
    fs::write(&note, "a change handed in from a process that may not remove files in its lane\n").unwrap();
    let arguments = ["--message", "Add the note handed in from a confined process", "--", note.to_str().unwrap()];
    let hand = queueing(&world, VERB, VERB_ARGUMENTS, &arguments).env(VERB_CONFINED, confined.parent().unwrap()).spawn().unwrap();
    let (output, journal) = decided(&world, &north, hand);
    let text = report(&output);
    assert_eq!(output.status.code(), Some(0), "{text}\n{journal}");
    assert!(text.contains("rebased the lane's 0 commit(s)") && !text.contains("stays on its base"), "a lane whose hand-in may not remove files in it was not rebased onto main before its checks: {text}");
    let kept = if confined.exists() { git(&confined, &["ls-files", "--others", "--exclude-standard"]) } else { String::new() };
    assert!(!confined.exists(), "a landed lane whose hand-in may not remove files in it was kept, holding untracked {kept:?}: {text}");
}

const PREQUEUE: &str = "hand_in::finish_runs_acceptance_inside_the_pre_queue_run";
const PREQUEUE_ARGUMENTS: &str = "NORTH_FINISH_PREQUEUE_IT_ARGUMENTS";

#[test]
fn finish_runs_acceptance_inside_the_pre_queue_run() {
    if let Ok(arguments) = std::env::var(PREQUEUE_ARGUMENTS) {
        let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
        let road = |lane: &hand_in::Lane| enqueue::run(&["--repo".to_owned(), lane.root.display().to_string()]);
        std::process::exit(i32::from(hand_in::finish(&arguments, road)));
    }
    let (world, north) = scanned_world(JUDGE_PASSES);
    write_executable(&world.path().join("bin/north"), VERDICT_NORTH);
    let (queued, accepted) = (world.path().join("queued-runs"), world.path().join("accepted-runs"));
    fs::write(north.checkout.join(".safe-push"), format!("[landing]\n\tpush-app = lander\n[land]\n\tcheck = ** :: echo ran >> {}\n", queued.display())).unwrap();
    git(&north.checkout, &["commit", "-qam", "Run a land check before queueing"]);
    git(&north.checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    git(&north.checkout, &["fetch", "-q", "origin"]);
    let (harness, harness_home) = (Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../harness")), world.path().join("harness-home"));
    fs::create_dir_all(harness_home.join("harness")).unwrap();
    for entry in fs::read_dir(harness).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() != "defaults.toml" {
            std::os::unix::fs::symlink(entry.path().canonicalize().unwrap(), harness_home.join("harness").join(entry.file_name())).unwrap();
        }
    }
    let defaults = fs::read_to_string(harness.join("defaults.toml")).unwrap().replace("[finish_acceptance_in_prequeue]\nvalue = \"off\"", "[finish_acceptance_in_prequeue]\nvalue = \"on\"");
    fs::write(harness_home.join("harness/defaults.toml"), defaults).unwrap();
    let store = north_base::platform::finish_record::FinishRecords(world.path().join("verdicts").join("finished"));
    let count = |path: &Path| fs::read_to_string(path).unwrap_or_default().lines().count();
    for (code, session) in [(3, "red"), (0, "green")] {
        let check = format!("grep -q 'the {session} note' note.txt && echo ran >> {} && exit {code}", accepted.display());
        let lane = worktree(&north, session);
        fs::write(lane.join("note.txt"), format!("the {session} note\n")).unwrap();
        let (queued_before, accepted_before) = (count(&queued), count(&accepted));
        let mut call = queueing(&world, PREQUEUE, PREQUEUE_ARGUMENTS, &["--check", &check, "--message", "Add the note (north#1036)", "--", lane.join("note.txt").to_str().unwrap()]);
        let output = call.env("NORTH_FLEET_SESSION", session).env("NORTH_VERDICT_CACHE_DIR", world.path().join("verdicts")).env("NORTH_HOME", &harness_home).output().unwrap();
        let text = report(&output);
        let runs: Vec<&str> = text.lines().filter(|line| line.contains("check(s) before queueing the lane") && line.contains(&check)).collect();
        assert!(matches!(runs[..], [run] if run.contains(&check)), "the acceptance command did not run as one more check of one run before queueing: {text}");
        assert_eq!((count(&queued) - queued_before, count(&accepted) - accepted_before), (1, 1), "a hand-in ran its checks before queueing or its acceptance command more than once: {text}");
        assert_eq!(output.status.code(), Some(i32::from(code != 0)), "{text}");
        let recorded = store.read(session).unwrap_or_else(|| panic!("the finish call wrote no finish record: {text}"));
        assert_eq!((recorded.passed, recorded.command.as_str()), (code == 0, check.as_str()), "the finish record is not the acceptance command's verdict: {text}");
    }
    assert_eq!(queue::pending(&north.queue).unwrap().len(), 1, "the passing call did not queue its lane, or the failing one did");

    let pinned = worktree(&north, "pinned");
    let marker = world.path().join("ran-here");
    let local = format!("touch {}", marker.display());
    fs::write(pinned.join(".safe-push"), format!("[landing]\n\tpush-app = lander\n[land]\n\tlocal-only = it reads this host :: {local}\n")).unwrap();
    git(&pinned, &["commit", "-qam", "Run a check only on this host"]);
    let (base, head) = (git(&pinned, &["rev-parse", "origin/main"]), git(&pinned, &["rev-parse", "HEAD"]));
    let judged = north_lander::land::slots::runner::judge_with_acceptance(&pinned, &base, &head, &local, Some("builder"), &north_base::platform::north_binary::land_north());
    assert_eq!(judged.accepted, north_lander::land::slots::runner::Accepted::Unjudged, "{judged:?}");
    assert!(judged.run.as_ref().is_err_and(|refusal| format!("{refusal:?}").contains("would run on this host rather than on builder")), "a run pinned to a build host started with a check that runs on this host: {judged:?}");
    assert!(!marker.exists(), "a run pinned to a build host ran a local-only check here");
}
