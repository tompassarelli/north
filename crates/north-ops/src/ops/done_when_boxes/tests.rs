use super::*;
use crate::platform::rolling_issue::Issue;

const FIXTURE: &str = "Fixture issue.\n\n## Done when\n- [ ] The marker ships. Done when: `cat marker` prints `shipped on main`.\n- [ ] The broken check stays open. Done when: `echo nope; exit 1` passes.\n- [ ] Prose only. Done when: the `north fleet` view shows it.\n- [x] Already done. Done when: `true`\n";

struct Fixture {
    read: String,
    edits: Vec<String>,
}

impl Tracker for Fixture {
    fn open(&self, _: &str) -> Result<Option<Issue>, String> {
        Ok(None)
    }

    fn body(&self, _: u64) -> Result<String, String> {
        Ok(self.read.clone())
    }

    fn listed(&self, _: &str) -> Result<Vec<(u64, String)>, String> {
        Ok(Vec::new())
    }

    fn create(&mut self, _: &str, _: &str) -> Result<(), String> {
        Err("no create".into())
    }

    fn edit(&mut self, _: u64, body: &str) -> Result<(), String> {
        self.edits.push(body.to_owned());
        Ok(())
    }

    fn close(&mut self, _: u64, _: &str) -> Result<(), String> {
        Err("no close".into())
    }
}

fn main_with_marker() -> TempDir {
    let main = tempfile::tempdir().unwrap();
    std::fs::write(main.path().join("marker"), "shipped on main\n").unwrap();
    main
}

fn stamp() -> Stamp {
    Stamp { sha: "0123456789abcdef0123".into(), date: "2026-11-05".into() }
}

#[test]
fn only_open_boxes_whose_done_when_starts_with_a_backticked_command_are_checks() {
    let found = checks(FIXTURE);
    assert_eq!(found.iter().map(|check| check.command.as_str()).collect::<Vec<_>>(), vec!["cat marker", "echo nope; exit 1"]);
}

#[test]
fn a_fixture_issue_gets_exactly_its_passing_box_ticked_with_the_output_attached() {
    let main = main_with_marker();
    let mut issue = Fixture { read: FIXTURE.into(), edits: Vec::new() };
    let verdicts = settle(&mut issue, 7, FIXTURE, &|command| judge(command, main.path()), &stamp(), false).unwrap();
    assert_eq!(verdicts.iter().map(|verdict| (verdict.outcome, verdict.run.exit)).collect::<Vec<_>>(), vec![(Outcome::Pass, 0), (Outcome::Fail, 1)]);
    assert_eq!(issue.edits.len(), 1);
    let edited = &issue.edits[0];
    let lines: Vec<&str> = edited.lines().collect();
    assert_eq!(lines[3], "- [x] The marker ships. Done when: `cat marker` prints `shipped on main`. Evidence: `cat marker` exited 0 on main 0123456789ab on 2026-11-05.");
    assert_eq!(&lines[4..7], ["  ```text", "  shipped on main", "  ```"]);
    assert_eq!(lines[7], "- [ ] The broken check stays open. Done when: `echo nope; exit 1` passes.");
    assert_eq!(edited.matches("- [x]").count(), 2);
    assert!(!edited.contains("nope\n"));
    assert!(edited.ends_with('\n'));
}

#[test]
fn the_write_reapplies_the_ticks_to_a_body_reread_after_the_checks_ran() {
    let main = main_with_marker();
    let reread = format!("{FIXTURE}- [ ] A box filed while the checks ran.\n");
    let mut issue = Fixture { read: reread, edits: Vec::new() };
    settle(&mut issue, 7, FIXTURE, &|command| judge(command, main.path()), &stamp(), false).unwrap();
    assert!(issue.edits[0].ends_with("- [ ] A box filed while the checks ran.\n"));
    assert!(issue.edits[0].contains("- [x] The marker ships."));
}

#[test]
fn a_box_ticked_by_hand_before_the_write_is_left_as_it_is() {
    let main = main_with_marker();
    let reread = FIXTURE.replace("- [ ] The marker ships.", "- [x] The marker ships.");
    let mut issue = Fixture { read: reread, edits: Vec::new() };
    settle(&mut issue, 7, FIXTURE, &|command| judge(command, main.path()), &stamp(), false).unwrap();
    assert!(issue.edits.is_empty());
}

#[test]
fn a_dry_run_prints_each_verdict_and_writes_nothing() {
    let main = main_with_marker();
    let mut issue = Fixture { read: FIXTURE.into(), edits: Vec::new() };
    let verdicts = settle(&mut issue, 7, FIXTURE, &|command| judge(command, main.path()), &stamp(), true).unwrap();
    assert!(issue.edits.is_empty());
    let printed: Vec<String> = verdicts.iter().map(|verdict| verdict.line("north#7")).collect();
    assert_eq!(
        printed,
        vec![
            "north#7: pass (exit 0) `cat marker`: The marker ships. Done when: `cat marker` prints `shipped on main`.",
            "north#7: fail (exit 1) `echo nope; exit 1`: The broken check stays open. Done when: `echo nope; exit 1` passes.",
        ]
    );
}

#[test]
fn commands_run_in_the_main_checkout_not_the_caller_directory() {
    let main = main_with_marker();
    assert_eq!(judge("test -f marker", main.path()).exit, 0);
    assert_ne!(judge("test -f marker", tempfile::tempdir().unwrap().path()).exit, 0);
    assert_ne!(judge("no-such-done-when-command-north255", main.path()).exit, 0);
}

#[test]
fn a_long_output_keeps_its_last_lines() {
    let output: String = (1..=30).map(|line| format!("line {line}\n")).collect();
    let block = attachment(&output);
    assert_eq!(block.len(), KEPT_LINES + 2);
    assert_eq!(block[1], "  line 11");
    assert!(attachment("  \n").is_empty());
}

#[test]
fn the_verb_takes_one_source_and_refuses_the_rest() {
    let words = |list: &[&str]| list.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
    let parsed = options(&words(&["--done-when", "--dry-run", "--repo", "north", "--main", "/tmp/main"])).unwrap();
    assert!(parsed.dry_run);
    assert_eq!(parsed.main, Some(PathBuf::from("/tmp/main")));
    assert!(options(&words(&["--done-when"])).is_err());
    assert!(options(&words(&["--done-when", "north#1", "--repo", "north"])).is_err());
    assert_eq!(issue_reference("north#255").unwrap(), ("tompassarelli/north".into(), 255));
}

#[test]
fn a_fetched_main_is_a_detached_checkout_of_the_newest_origin_main() {
    let root = tempfile::tempdir().unwrap();
    let sh = |dir: &Path, script: &str| assert!(Command::new("sh").args(["-c", script]).current_dir(dir).status().unwrap().success(), "{script}");
    let identity = "git -c user.name=t -c user.email=t@example.invalid";
    sh(root.path(), &format!("git init -q -b main origin && cd origin && {identity} commit -q --allow-empty -m one && cd .. && git clone -q origin clone"));
    sh(root.path(), &format!("cd origin && echo two > marker && git add marker && {identity} commit -q -m two"));
    let clone = root.path().join("clone");
    let main = Main::fetch(&clone).unwrap();
    let newest = String::from_utf8(Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root.path().join("origin")).output().unwrap().stdout).unwrap();
    assert_eq!(main.sha().unwrap(), newest.trim());
    let run = judge("cat marker", main.dir());
    assert_eq!((run.exit, run.stdout.as_str()), (0, "two\n"));
    let checkout = main.dir().to_owned();
    drop(main);
    assert!(!checkout.exists());
}

fn verdict_of(line: &str, main: &Path) -> (Outcome, String) {
    let found = verdicts(line, &|command| judge(command, main));
    assert_eq!(found.len(), 1, "{line}");
    (found[0].outcome, found[0].line("north#360"))
}

fn git_main_with(files: &[&str]) -> TempDir {
    let main = tempfile::tempdir().unwrap();
    for file in files {
        let path = main.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\n").unwrap();
    }
    assert!(Command::new("sh").args(["-c", "git init -q && git add -A"]).current_dir(main.path()).status().unwrap().success());
    main
}

const SAFE_PUSH_BOX: &str = "- [ ] bin/safe-push.test.sh is retired. Done when: `git ls-files bin/safe-push.test.sh` prints nothing.";

#[test]
fn a_command_that_must_print_nothing_fails_while_the_file_it_lists_is_still_on_main() {
    let still_there = git_main_with(&["bin/safe-push.test.sh"]);
    assert_eq!(judge("git ls-files bin/safe-push.test.sh", still_there.path()).exit, 0);
    assert_eq!(verdict_of(SAFE_PUSH_BOX, still_there.path()).0, Outcome::Fail);
    let retired = git_main_with(&["bin/safe-push"]);
    assert_eq!(verdict_of(SAFE_PUSH_BOX, retired.path()).0, Outcome::Pass);
}

#[test]
fn a_search_that_must_find_no_match_passes_on_rg_or_grep_exit_one_and_fails_on_a_match_or_an_error() {
    let main = main_with_marker();
    let line = |pattern: &str| format!("- [ ] Placement is gone. Done when: `grep -r '{pattern}' .` finds no match.");
    assert_eq!(verdict_of(&line("placement picks"), main.path()).0, Outcome::Pass);
    assert_eq!(verdict_of(&line("shipped"), main.path()).0, Outcome::Fail);
    assert_eq!(verdict_of("- [ ] Gone. Done when: `no-such-command-north255` prints nothing.", main.path()).0, Outcome::Fail);
}

#[test]
fn exit_zero_counts_only_when_the_box_says_the_command_passes_succeeds_or_exits_zero() {
    let main = main_with_marker();
    for said in ["passes", "succeeds", "exits 0", "is green"] {
        assert_eq!(verdict_of(&format!("- [ ] Box. Done when: `true` {said}."), main.path()).0, Outcome::Pass, "{said}");
        assert_eq!(verdict_of(&format!("- [ ] Box. Done when: `false` {said}."), main.path()).0, Outcome::Fail, "{said}");
    }
    assert_eq!(verdict_of("- [ ] Box. Done when: `exit 3` exits 3.", main.path()).0, Outcome::Pass);
    let (bare, line) = verdict_of("- [ ] Box. Done when: `true`", main.path());
    assert_eq!(bare, Outcome::Judge);
    assert!(line.contains("judge (exit 0)") && line.ends_with("[no stated expectation]"), "{line}");
    assert_eq!(verdict_of("- [ ] Box. Done when: `true` and the reader agrees.", main.path()).0, Outcome::Judge);
}

#[test]
fn a_box_that_says_the_command_prints_or_shows_a_text_needs_that_text_in_the_output() {
    let main = main_with_marker();
    assert_eq!(verdict_of("- [ ] Box. Done when: `cat marker` prints `shipped on main`.", main.path()).0, Outcome::Pass);
    assert_eq!(verdict_of("- [ ] Box. Done when: `cat marker` shows \"shipped\" and exits 0.", main.path()).0, Outcome::Pass);
    assert_eq!(verdict_of("- [ ] Box. Done when: `cat marker` prints `landed on main`.", main.path()).0, Outcome::Fail);
    assert_eq!(verdict_of("- [ ] Box. Done when: `cat marker` shows shipped.", main.path()).0, Outcome::Pass);
    assert_eq!(verdict_of("- [ ] Box. Done when: `echo ok >&2; exit 1` prints `ok` and passes.", main.path()).0, Outcome::Fail);
}

#[test]
fn a_threshold_or_comparison_the_program_cannot_read_is_a_judge_verdict_and_is_never_ticked() {
    let main = main_with_marker();
    for line in [
        "- [ ] Fast. Done when: `echo 3` prints a count of at most 5.",
        "- [ ] Fast. Done when: `echo 200` shows a median under 240 s.",
        "- [ ] Fast. Done when: `echo 2` prints a number < 5.",
        "- [ ] Fast. Done when: `true` passes within 25% of last week.",
    ] {
        let (outcome, printed) = verdict_of(line, main.path());
        assert_eq!(outcome, Outcome::Judge, "{line}");
        assert!(printed.contains("threshold"), "{printed}");
    }
    let body = "- [ ] Fast. Done when: `echo 3` prints a count of at most 5.\n";
    let mut issue = Fixture { read: body.into(), edits: Vec::new() };
    settle(&mut issue, 7, body, &|command| judge(command, main.path()), &stamp(), false).unwrap();
    assert!(issue.edits.is_empty());
}
