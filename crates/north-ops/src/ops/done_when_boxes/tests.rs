use super::*;
use crate::platform::rolling_issue::Issue;

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

#[test]
fn a_command_that_must_print_nothing_fails_while_the_file_it_lists_is_still_on_main() {
    let still_there = git_main_with(&["bin/safe-push.test.sh"]);
    assert_eq!(judge("git ls-files bin/safe-push.test.sh", still_there.path()).exit, 0);
    assert_eq!(verdict_of(SAFE_PUSH_BOX, still_there.path()).0, Outcome::Fail);
    let retired = git_main_with(&["bin/safe-push"]);
    assert_eq!(verdict_of(SAFE_PUSH_BOX, retired.path()).0, Outcome::Pass);
}
