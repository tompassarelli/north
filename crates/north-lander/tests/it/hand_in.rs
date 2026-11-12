use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Output, Stdio};

use north_base::foundation::shim::write_executable;
use north_base::platform::wait;
use north_lander::land::{enqueue, hand_in, queue};

use super::land_first::{NO_SECRETS, PUBLIC_REPO, queueing};
use super::serve::{HANG, Repository, World, git, report};

const VERB: &str = "hand_in::a_hand_in_verb_call_lands_named_paths_staged_now_or_committed_first";
const VERB_ARGUMENTS: &str = "NORTH_HAND_IN_IT_ARGUMENTS";
const ROAD: &str = "serve::enqueue_process";
const ROAD_ARGUMENTS: &str = "NORTH_LAND_IT_ARGUMENTS";
const JUDGE_PASSES: &str = "#!/bin/sh\ncat > /dev/null\nexit 0\n";
const VERDICT_NORTH: &str = r#"#!/bin/sh
cache="$XDG_STATE_HOME/north/verdict-cache"
case "$1 $2 $3 $4" in
  'capacity deadline '*) shift 5; exec "$@" ;;
  'capacity offload verdict hit') shift 4; for key; do [ -f "$cache/$key" ] && printf '%s local %s\n' "$key" "$(cat "$cache/$key")"; done; exit 0 ;;
  'capacity offload verdict record') shift 4; while [ $# -gt 0 ]; do case "$1" in --key) key=$2; shift ;; --pass) pass=$2; shift ;; esac; shift; done; [ -z "$pass" ] || { mkdir -p "$cache" && echo "$(date +%s) $pass" > "$cache/$key"; }; exit 0 ;;
esac
exit 1
"#;
const JUDGE_FINDS_AN_ADDRESS: &str = "#!/bin/sh\ntext=$(cat)\ncase \"$text\" in\n  *'lives at'*) printf 'address\\tthe owner lives at 1 Example Road\\n'; exit 1 ;;\nesac\nexit 0\n";
const JUDGE_ASKS_A_MODEL: &str = "#!/bin/sh\ncat > /dev/null\nsleep 1\nexit 0\n";

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

fn safe_push_road(world: &World, lane: &Path) -> Child {
    queueing(world, ROAD, ROAD_ARGUMENTS, &["--repo", lane.to_str().unwrap(), "--wait"]).spawn().unwrap()
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
fn a_hand_in_verb_call_lands_named_paths_staged_now_or_committed_first() {
    if let Ok(arguments) = std::env::var(VERB_ARGUMENTS) {
        let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
        let road = |lane: &hand_in::Lane| enqueue::run(&["--repo".to_owned(), lane.root.display().to_string(), "--wait".to_owned()]);
        std::process::exit(i32::from(hand_in::run(&arguments, road)));
    }
    let (world, north) = scanned_world(JUDGE_PASSES);
    let lane = worktree(&north, "verb");
    fs::write(lane.join("named.txt"), "the named change\n").unwrap();
    fs::write(lane.join("unnamed.txt"), "a change the worker did not name\n").unwrap();
    let start = git(&lane, &["rev-parse", "HEAD"]);

    for (paths, refusal) in [([lane.join("named.txt"), north.checkout.join("README")], "is outside the lane"), ([lane.join("README"), lane.join("README")], "no changes in")] {
        let output = verb(&world, "Add the named note", &[&paths[0], &paths[1]]).wait_with_output().unwrap();
        let text = report(&output);
        assert_eq!(output.status.code(), Some(1), "{text}");
        assert!(text.contains(refusal) && text.contains("NOT committed"), "{text}");
        assert_eq!(git(&lane, &["rev-parse", "HEAD"]), start, "a refused hand-in committed: {text}");
        assert!(queue::pending(&north.queue).unwrap().is_empty(), "a refused hand-in queued: {text}");
    }

    let (output, journal) = decided(&world, &north, verb(&world, "Add the named note (north#1158)", &[&lane.join("named.txt")]));
    let text = report(&output);
    let landed = git(&north.origin, &["rev-parse", "main"]);
    assert_eq!(output.status.code(), Some(0), "{text}\n{journal}");
    assert!(text.contains(&format!("Done: landed {landed} on main")), "the verb did not return the lander's verdict: {text}\n{journal}");
    assert_eq!(git(&north.origin, &["log", "-1", "--format=%s", "main"]), "Add the named note (north#1158)");
    assert_eq!(git(&north.origin, &["diff-tree", "--no-commit-id", "--name-only", "-r", "main"]), "named.txt", "the verb committed more than the named path");

    git(&north.checkout, &["fetch", "-q", "origin"]);
    let lane = worktree(&north, "committed");
    fs::write(lane.join("committed.txt"), "the committed change\n").unwrap();
    git(&lane, &["add", "committed.txt"]);
    git(&lane, &["commit", "-qm", "Commit the change first"]);
    fs::write(lane.join("other.txt"), "a change committed beside it\n").unwrap();
    git(&lane, &["add", "other.txt"]);
    git(&lane, &["commit", "-qm", "Commit another change"]);
    let start = git(&lane, &["rev-parse", "HEAD"]);
    let base = git(&north.origin, &["rev-parse", "main"]);

    let refused = verb(&world, "Add the committed note", &[&lane.join("committed.txt")]).wait_with_output().unwrap();
    let text = report(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(text.contains("other.txt") && text.contains("NOT committed"), "{text}");
    assert_eq!(git(&lane, &["rev-parse", "HEAD"]), start, "a refused hand-in committed: {text}");
    assert!(queue::pending(&north.queue).unwrap().is_empty(), "a refused hand-in queued: {text}");

    let (output, journal) = decided(&world, &north, verb(&world, "Add the committed note (north#1158)", &[&lane.join("committed.txt"), &lane.join("other.txt")]));
    let text = report(&output);
    let committed_landed = git(&north.origin, &["rev-parse", "main"]);
    assert_eq!(output.status.code(), Some(0), "{text}\n{journal}");
    assert!(text.contains(&format!("Done: landed {committed_landed} on main")), "the verb did not queue the committed lane: {text}\n{journal}");
    assert_eq!(git(&north.origin, &["diff", "--name-only", &base, &committed_landed]), "committed.txt\nother.txt", "the lander did not land exactly the committed paths");
}

fn refused_by_both_roads(world: &World, north: &Repository, lane: &Path, path: &Path, refusal: &str) {
    let by_verb = verb(world, "Hand in the refused change", &[path]).wait_with_output().unwrap();
    let by_safe_push = safe_push_road(world, lane).wait_with_output().unwrap();
    assert_eq!((by_verb.status.code(), by_safe_push.status.code()), (Some(1), Some(1)), "{}\n{}", report(&by_verb), report(&by_safe_push));
    assert!(refusals(&by_verb).iter().any(|line| line.contains(refusal)), "{}", report(&by_verb));
    assert_eq!(refusals(&by_verb), refusals(&by_safe_push), "a gate refused the lane in other words through the verb");
    assert!(queue::pending(&north.queue).unwrap().is_empty(), "a refused lane was queued");
}

#[test]
fn a_hand_in_verb_call_returns_the_same_refusal_as_safe_push_for_a_refused_lane() {
    let (world, north) = scanned_world(JUDGE_FINDS_AN_ADDRESS);
    let lane = worktree(&north, "conflict");
    fs::write(north.checkout.join("README"), "main moved on\n").unwrap();
    git(&north.checkout, &["commit", "-qam", "Main rewrites the README"]);
    git(&north.checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    fs::write(lane.join("README"), "the lane rewrites it too\n").unwrap();
    refused_by_both_roads(&world, &north, &lane, &lane.join("README"), "conflicts with origin/main");

    let lane = worktree(&north, "private");
    fs::write(lane.join("private.txt"), "the owner lives at 1 Example Road\n").unwrap();
    refused_by_both_roads(&world, &north, &lane, &lane.join("private.txt"), "PRIVATE KIND");
}

const FINISH: &str = "hand_in::a_finish_call_queues_only_after_its_check_passes_and_returns_the_ticket_before_the_verdict";
const FINISH_ARGUMENTS: &str = "NORTH_FINISH_IT_ARGUMENTS";

fn finish(world: &World, session: &str, check: &str, message: &str, path: &Path) -> Output {
    let arguments = ["--check", check, "--message", message, "--", path.to_str().unwrap()];
    queueing(world, FINISH, FINISH_ARGUMENTS, &arguments).env("NORTH_FLEET_SESSION", session).output().unwrap()
}

#[test]
fn a_finish_call_queues_only_after_its_check_passes_and_returns_the_ticket_before_the_verdict() {
    if let Ok(arguments) = std::env::var(FINISH_ARGUMENTS) {
        let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
        let road = |lane: &hand_in::Lane| enqueue::run(&["--repo".to_owned(), lane.root.display().to_string()]);
        std::process::exit(i32::from(hand_in::finish(&arguments, road)));
    }
    let (world, north) = scanned_world(JUDGE_ASKS_A_MODEL);
    write_executable(&world.path().join("bin/north"), VERDICT_NORTH);
    fs::write(north.checkout.join(".safe-push"), "[landing]\n\tpush-app = lander\n[land]\n\tcheck = ** :: sleep 3\n").unwrap();
    git(&north.checkout, &["commit", "-qam", "Run a land check before queueing"]);
    git(&north.checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    git(&north.checkout, &["fetch", "-q", "origin"]);
    let lane = worktree(&north, "finish");
    fs::write(lane.join("named.txt"), "the finished change\n").unwrap();
    fs::write(lane.join("unnamed.txt"), "a change the worker did not name\n").unwrap();
    let start = git(&lane, &["rev-parse", "HEAD"]);

    let failed = finish(&world, "proposer", "echo the box is red; exit 3", "Add the finished note (north#1194)", &lane.join("named.txt"));
    let text = report(&failed);
    assert_eq!(failed.status.code(), Some(1), "{text}");
    assert!(text.contains("the box is red") && text.contains("exited 3") && text.contains("NOT queued"), "a failed check did not return its output: {text}");
    assert_eq!(git(&lane, &["rev-parse", "HEAD"]), start, "a failed check committed: {text}");
    assert_eq!(git(&lane, &["diff", "--cached", "--name-only"]), "", "a failed check staged: {text}");
    assert!(queue::pending(&north.queue).unwrap().is_empty(), "a failed check queued: {text}");

    let mut proposing = queueing(&world, FINISH, FINISH_ARGUMENTS, &["--judge-message", "proposer"]).current_dir(&lane).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    proposing.stdin.take().unwrap().write_all(b"Add the proposed note (north#1239)").unwrap();
    let proposed = proposing.wait_with_output().unwrap();
    assert!(String::from_utf8_lossy(&proposed.stdout).split_whitespace().any(|word| word == hand_in::PROPOSED), "the launch-time scan did not pass the proposed message on to its judge: {}", report(&proposed));
    let stale = finish(&world, "later", "true", "", &lane.join("named.txt"));
    assert_eq!(stale.status.code(), Some(1), "another session's finish call committed the proposal kept for an earlier one: {}", report(&stale));
    assert!(queue::pending(&north.queue).unwrap().is_empty(), "another session's empty-message finish call queued: {}", report(&stale));
    let ran = world.path().join("check-ran");
    let passed = finish(&world, "proposer", &format!("test -e named.txt && touch {}", ran.display()), "", &lane.join("named.txt"));
    let text = report(&passed);
    assert_eq!(passed.status.code(), Some(0), "{text}");
    assert!(ran.exists(), "the check did not run in the lane: {text}");
    let pending = queue::pending(&north.queue).unwrap();
    let [(ticket, queued)] = &pending[..] else { panic!("the passing call did not leave one undecided ticket: {pending:?}\n{text}") };
    let name = ticket.file_name().unwrap().to_str().unwrap();
    assert!(text.lines().any(|line| line.starts_with("Done: queued") && line.ends_with(&format!("ticket {name}"))), "the call did not return its ticket: {text}");
    assert_eq!(git(&lane, &["diff-tree", "--no-commit-id", "--name-only", "-r", &queued.head]), "named.txt", "the call committed more than the named path");
    assert_eq!(git(&lane, &["log", "-1", "--format=%B", &queued.head]).trim_end(), "Add the proposed note (north#1239)", "a finish call that left out its message did not commit the proposed one byte for byte");
    assert_eq!(git(&lane, &["status", "--porcelain", "--", "unnamed.txt"]), "?? unnamed.txt", "the call swept up a path it was not given");
    assert_eq!((queued.alone_green.as_deref(), queued.unjudged_on.as_deref()), (Some(queued.head.as_str()), None), "the judge answered while the checks before queueing ran, yet the ticket waits for the lander to ask again (north#1239): {text}");
}

const FILES: &str = "hand_in::a_finish_call_writes_its_files_only_inside_its_own_lane_and_leaves_them_written_when_its_check_fails";
const FILES_ARGUMENTS: &str = "NORTH_FINISH_FILES_IT_ARGUMENTS";

fn finish_files(world: &World, lane: &Path, check: &str, files: &[(PathBuf, serde_json::Value)]) -> Output {
    let arguments = ["--check", check, "--message", "Write the finished files (north#1218)", "--files", "-"];
    let mut call = queueing(world, FILES, FILES_ARGUMENTS, &arguments).current_dir(lane).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let files: Vec<serde_json::Value> = files.iter().map(|(path, change)| serde_json::json!({"path": path}).as_object().unwrap().clone().into_iter().chain(change.as_object().unwrap().clone()).collect()).collect();
    call.stdin.take().unwrap().write_all(serde_json::Value::from(files).to_string().as_bytes()).unwrap();
    call.wait_with_output().unwrap()
}

fn whole(content: &str) -> serde_json::Value {
    serde_json::json!({"content": content})
}

fn lines(replacements: &[(&str, &str)]) -> serde_json::Value {
    serde_json::json!({"replacements": replacements.iter().map(|(old, new)| serde_json::json!({"old": old, "new": new})).collect::<Vec<_>>()})
}

#[test]
fn a_finish_call_writes_its_files_only_inside_its_own_lane_and_leaves_them_written_when_its_check_fails() {
    if let Ok(arguments) = std::env::var(FILES_ARGUMENTS) {
        let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
        let road = |lane: &hand_in::Lane| enqueue::run(&["--repo".to_owned(), lane.root.display().to_string()]);
        std::process::exit(i32::from(hand_in::finish(&arguments, road)));
    }
    let (world, north) = scanned_world(JUDGE_PASSES);
    let lane = worktree(&north, "files");
    let start = git(&lane, &["rev-parse", "HEAD"]);
    let (inside, outside, readme) = (lane.join("notes/new.txt"), north.checkout.join("outside.txt"), lane.join("README"));
    let first_readme = fs::read_to_string(&readme).unwrap();

    let refused = finish_files(&world, &lane, "true", &[(inside.clone(), whole("inside\n")), (outside.clone(), whole("outside\n"))]);
    let text = report(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(text.contains(&format!("{} is outside the lane", outside.display())), "{text}");
    assert!(!inside.exists() && !outside.exists(), "a refused call wrote a file: {text}");

    let absent = finish_files(&world, &lane, "true", &[(inside.clone(), whole("inside\n")), (readme.clone(), lines(&[(first_readme.as_str(), "kept\n"), ("text the readme lacks", "")]))]);
    let text = report(&absent);
    assert_eq!(absent.status.code(), Some(1), "{text}");
    assert!(text.contains(&format!("{}: the old text of replacement 2 is absent", readme.display())), "{text}");
    assert!(!inside.exists() && fs::read_to_string(&readme).unwrap() == first_readme, "a call with an absent old text wrote a file: {text}");

    let failed = finish_files(&world, &lane, "cat notes/new.txt; exit 3", &[(inside.clone(), whole("the red draft\n"))]);
    let text = report(&failed);
    assert_eq!(failed.status.code(), Some(1), "{text}");
    assert!(text.contains("the red draft") && text.contains("exited 3") && text.contains("NOT queued"), "the check did not run on the written file: {text}");
    assert_eq!(fs::read_to_string(&inside).unwrap(), "the red draft\n", "a failed check did not leave the file as written");
    assert_eq!(git(&lane, &["rev-parse", "HEAD"]), start, "a failed check committed: {text}");
    assert!(queue::pending(&north.queue).unwrap().is_empty(), "a failed check queued: {text}");

    let ambiguous = finish_files(&world, &lane, "true", &[(readme.clone(), whole("the rewritten readme\n")), (inside.clone(), lines(&[("d", "")]))]);
    let text = report(&ambiguous);
    assert_eq!(ambiguous.status.code(), Some(1), "{text}");
    assert!(text.contains(&format!("{}: the old text of replacement 1 occurs 2 times", inside.display())), "{text}");
    assert!(fs::read_to_string(&inside).unwrap() == "the red draft\n" && fs::read_to_string(&readme).unwrap() == first_readme, "a call with an ambiguous old text wrote a file: {text}");

    let both = serde_json::json!({"content": "the whole draft\n", "replacements": [{"old": "red draft", "new": "green note"}]});
    let mixed = finish_files(&world, &lane, "true", &[(readme.clone(), whole("the rewritten readme\n")), (inside.clone(), both)]);
    let text = report(&mixed);
    assert_eq!(mixed.status.code(), Some(1), "{text}");
    assert!(text.contains(&format!("{}: carries both whole content and replacements", inside.display())), "{text}");
    assert!(fs::read_to_string(&inside).unwrap() == "the red draft\n" && fs::read_to_string(&readme).unwrap() == first_readme, "a call with an entry carrying both forms wrote a file: {text}");

    write_executable(&world.path().join("bin/north"), VERDICT_NORTH);
    let runs = world.path().join("runs");
    let (config, count) = (whole("[landing]\n\tpush-app = lander\n[land]\n\tcheck = ** :: sh count.sh\n"), whole(&format!("grep -q green notes/new.txt || exit 1\necho ran >> {}\n", runs.display())));
    let passed = finish_files(&world, &lane, "sh count.sh", &[(inside, lines(&[("red draft", "green note")])), (readme, whole("the rewritten readme\n")), (lane.join(".safe-push"), config), (lane.join("count.sh"), count)]);
    let text = report(&passed);
    assert_eq!(passed.status.code(), Some(0), "{text}");
    assert_eq!(fs::read_to_string(&runs).unwrap_or_default().lines().count(), 1, "the finish call's check, one of the lane's checks before queueing, ran again before the ticket: {text}");
    let pending = queue::pending(&north.queue).unwrap();
    let [(_, queued)] = &pending[..] else { panic!("the passing call did not leave one undecided ticket: {pending:?}\n{text}") };
    assert_eq!(git(&lane, &["diff-tree", "--no-commit-id", "--name-only", "-r", &queued.head]), ".safe-push\nREADME\ncount.sh\nnotes/new.txt", "the call did not commit exactly its files");
    assert_eq!(git(&lane, &["show", &format!("{}:notes/new.txt", queued.head)]), "the green note");
}

#[test]
fn a_hand_in_verb_call_stages_a_deleted_file_or_the_deletions_under_a_named_directory() {
    if let Ok(arguments) = std::env::var(VERB_ARGUMENTS) {
        let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
        let road = |lane: &hand_in::Lane| enqueue::run(&["--repo".to_owned(), lane.root.display().to_string(), "--wait".to_owned()]);
        std::process::exit(i32::from(hand_in::run(&arguments, road)));
    }
    let (world, north) = scanned_world(JUDGE_PASSES);
    fs::create_dir_all(north.checkout.join("old/inner")).unwrap();
    for file in ["old/a.txt", "old/inner/b.txt", "kept.txt"] {
        fs::write(north.checkout.join(file), "to be removed\n").unwrap();
    }
    git(&north.checkout, &["add", "old", "kept.txt"]);
    git(&north.checkout, &["commit", "-qm", "Add files to remove"]);
    git(&north.checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let lane = worktree(&north, "removal");
    fs::remove_file(lane.join("README")).unwrap();
    fs::remove_dir_all(lane.join("old")).unwrap();
    fs::remove_file(lane.join("kept.txt")).unwrap();
    let start = git(&lane, &["rev-parse", "HEAD"]);

    let refused = verb(&world, "Remove what never existed", &[&lane.join("never/existed.txt")]).wait_with_output().unwrap();
    let text = report(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(text.contains("never/existed.txt") && text.contains("not in the lane") && text.contains("NOT committed"), "{text}");
    assert_eq!(git(&lane, &["rev-parse", "HEAD"]), start, "a refused hand-in committed: {text}");

    let (output, journal) = decided(&world, &north, verb(&world, "Remove the old files (north#1268)", &[&lane.join("README"), &lane.join("old")]));
    let text = report(&output);
    let landed = git(&north.origin, &["rev-parse", "main"]);
    assert_eq!(output.status.code(), Some(0), "{text}\n{journal}");
    assert!(text.contains(&format!("Done: landed {landed} on main")), "{text}\n{journal}");
    assert_eq!(git(&north.origin, &["diff-tree", "--no-commit-id", "--name-status", "-r", "main"]), "D\tREADME\nD\told/a.txt\nD\told/inner/b.txt", "the deletions did not land, or more than the named paths did");
    assert_eq!((git(&lane, &["diff", "--cached", "--name-only"]), git(&lane, &["ls-files", "--deleted"])), (String::new(), "kept.txt".to_owned()), "an unnamed deletion was staged");
}

#[test]
fn a_hand_in_verb_call_takes_a_named_path_literally_so_a_glob_or_pathspec_magic_stages_only_that_name() {
    let (world, north) = scanned_world(JUDGE_PASSES);
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
