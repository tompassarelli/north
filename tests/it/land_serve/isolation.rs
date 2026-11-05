use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_landing::land::queue::RedFix;

use super::{Fixture, executable, git, tree};

fn worktree_state(root: &Path) -> (String, String) {
    (git(root, &["rev-parse", "HEAD"]), git(root, &["status", "--porcelain"]))
}

#[test]
fn serve_tells_the_culprit_its_own_refusal_reason() {
    let mut fixture = Fixture::new();
    let tickets: Vec<PathBuf> = ["a", "b"].iter().map(|name| {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None)
    }).collect();
    executable(
        &fixture.trace.join("hook"),
        r#"#!/usr/bin/env bash
command git cat-file -e HEAD:b.txt 2>/dev/null || exit 0
echo refused >"$LAND_TRACE/verdict-$1"
for k in $(seq 300); do echo "   Compiling crate-$k v0.1.0"; done
echo 'dep-gate: crates/x/Cargo.toml: proptest: added dependency needs a reason'
printf 'error[E0425]: cannot find value `lane_b` in this scope\n --> src/lib.rs:2:5\n\n'
echo 'running 2 tests'
printf -- '---- lane_b::adds stdout ----\nthread %s panicked at src/lib.rs:4:9:\nassertion `left == right` failed in check %s\n  left: 1\n right: 2\nnote: run with `RUST_BACKTRACE=1`\n' "'lane_b::adds'" "$1"
echo 'safe-push: land check cargo test exited 101 after 12s'
echo "safe-push: ✗ the pre-push checks failed in check $1; NOT pushed."
echo "safe-push: ✗ the pre-push checks failed in check $1; NOT pushed."
"#,
    );
    assert!(fixture.serve().status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 4, "the refused candidate of a and b, each lane alone, then a on its own");
    let n = 1 + (1..3).find(|index| Command::new("git").arg("-C").arg(&fixture.repo).args(["cat-file", "-e", &format!("{}:b.txt", checks[*index].0)]).output().unwrap().status.success()).unwrap();
    assert_eq!(fixture.verdict(&tickets[0]), Some(format!("landed {}", fixture.main())));
    let failure = fixture.verdict(&tickets[1]).unwrap();
    let fleet = fixture.fleet();
    let told: Vec<&String> = fleet.iter().filter(|call| call.starts_with("fleet send --from service:north-land session-lane-b ")).collect();
    assert_eq!(told.len(), 1, "{fleet:?}");
    let lines: Vec<&str> = told[0].trim_start_matches("fleet send --from service:north-land session-lane-b ").split("\\n").collect();
    let log = lines.last().unwrap().strip_prefix("full log: ").unwrap();
    assert!(log.ends_with("-candidate.log") && fs::read_to_string(log).unwrap().contains(&format!("check {n}")), "the full log is the culprit's own check: {log}");
    assert_eq!(
        lines[..9],
        [
            format!("north land: lane-b ({}) did not land: {failure}", git(&fixture.root.join("proj/worktrees/b"), &["rev-parse", "HEAD"])).as_str(),
            "dep-gate: crates/x/Cargo.toml: proptest: added dependency needs a reason",
            "error[E0425]: cannot find value `lane_b` in this scope",
            " --> src/lib.rs:2:5",
            "---- lane_b::adds stdout ----",
            "thread 'lane_b::adds' panicked at src/lib.rs:4:9:",
            format!("assertion `left == right` failed in check {n}").as_str(),
            "safe-push: land check cargo test exited 101 after 12s",
            format!("safe-push: ✗ the pre-push checks failed in check {n}; NOT pushed.").as_str(),
        ]
    );
    let tail = lines.iter().position(|line| *line == "last lines of the log:").expect("the log tail follows the errors");
    assert_eq!(lines[tail - 1], "safe-push: ✗ the pre-push checks failed in check 3; NOT pushed.");
    let tail_lines = &lines[tail + 1..lines.len() - 1];
    assert!(tail_lines.contains(&"running 2 tests") && tail_lines.contains(&"  left: 1") && tail_lines.len() <= north_landing::land::refusal::TAIL_LINES, "{tail_lines:?}");
    assert!(!tail_lines.iter().any(|line| line.starts_with("   Compiling crate-1 ")), "{tail_lines:?}");
    let reason: usize = lines[1..lines.len() - 1].iter().map(|line| line.len() + 1).sum();
    assert!(reason <= north_landing::land::refusal::BOUND + 1, "{reason}");
}

#[test]
fn serve_mixed() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = [("a", "a.txt"), ("b", "shared.txt"), ("c", "shared.txt"), ("d", "fail.txt"), ("e", "e.txt")]
        .iter()
        .map(|(name, file)| fixture.lane(name, file, name))
        .collect();
    fs::write(lanes[2].join("scratch.txt"), "untracked\n").unwrap();
    let before: Vec<_> = lanes.iter().map(|lane| worktree_state(lane)).collect();
    let tickets: Vec<PathBuf> = lanes.iter().map(|lane| fixture.enqueue(lane, None)).collect();
    git(&fixture.repo, &["cherry-pick", &git(&lanes[4], &["rev-parse", "HEAD"])]);
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    let e_landed = fixture.main();
    executable(
        &fixture.trace.join("hook"),
        "#!/usr/bin/env bash\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo 'refused https://github.com/o/r/actions/runs/4242' >\"$LAND_TRACE/verdict-$1\"\n",
    );
    assert!(fixture.serve().status.success());
    let main = fixture.main();
    assert_eq!(fixture.verdict(&tickets[4]), Some(format!("landed {e_landed}")), "a lane already on main counts as landed");
    assert_eq!(fixture.verdict(&tickets[0]), Some(format!("landed {main}")));
    assert_eq!(fixture.verdict(&tickets[1]), Some(format!("landed {main}")));
    assert_eq!(fixture.verdict(&tickets[2]).as_deref(), Some("conflict shared.txt"));
    assert_eq!(fixture.verdict(&tickets[3]).as_deref(), Some("failed https://github.com/o/r/actions/runs/4242"));
    assert_eq!(git(&fixture.origin, &["show", "main:shared.txt"]), "b");
    git(&fixture.origin, &["cat-file", "-e", "main:a.txt"]);
    assert!(git(&fixture.origin, &["ls-tree", "--name-only", "main"]).lines().all(|file| file != "fail.txt"), "the failing lane landed");
    let checks = fixture.checks();
    assert_eq!(checks.len(), 5, "the refused candidate of a, b and d, each lane alone, then a and b together");
    assert!(checks.iter().any(|(checked, _)| *checked == main), "main is a commit the checks passed");
    let fleet = fixture.fleet();
    assert!(fleet.iter().any(|line| line.starts_with("fleet send --from service:north-land session-lane-d ") && line.contains("did not land") && line.contains("https://github.com/o/r/actions/runs/4242")), "{fleet:?}");
    assert!(fleet.iter().any(|line| line.starts_with("fleet send --from service:north-land session-lane-c ") && line.contains("did not land") && line.contains("shared.txt")), "{fleet:?}");
    for (lane, sha) in [("a", &main), ("b", &main), ("e", &e_landed)] {
        assert!(fleet.contains(&format!("fleet send --from service:north-land session-lane-{lane} landed lane-{lane} as {sha}")), "{fleet:?}");
    }
    assert_eq!(fleet.len(), 5, "each lane is told once: {fleet:?}");
    for landed in [0, 1, 4] {
        assert!(!lanes[landed].exists(), "a landed clean lane is removed");
    }
    for kept in [2, 3] {
        assert_eq!(worktree_state(&lanes[kept]), before[kept], "the lander touches only its own worktree and the lanes it landed");
    }
}

#[test]
fn serve_lands_a_lane_whose_only_conflict_was_a_refused_lane_ahead_of_it() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "app.txt", "from a\n");
    fixture.commit(&a, "fail.txt", "fail\n", "fail marker from a");
    let b = fixture.lane("b", "app.txt", "from b\n");
    let tickets = [fixture.enqueue(&a, None), fixture.enqueue(&b, None)];
    executable(
        &fixture.trace.join("hook"),
        "#!/usr/bin/env bash\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo 'refused https://github.com/o/r/actions/runs/4343' >\"$LAND_TRACE/verdict-$1\"\n",
    );
    let output = fixture.serve();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("deferred: lane-b conflicts only with lanes ahead of it in this candidate (app.txt)"));
    assert_eq!(fixture.verdict(&tickets[0]).as_deref(), Some("failed https://github.com/o/r/actions/runs/4343"));
    assert_eq!(fixture.verdict(&tickets[1]), Some(format!("landed {}", fixture.main())));
    assert_eq!(git(&fixture.origin, &["show", "main:app.txt"]), "from b");
}

#[test]
fn serve_captured_candidate() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "new.py", "print('a')\n");
    let b = fixture.lane("b", "clean.txt", "clean\n");
    let bad = fixture.enqueue(&a, None);
    let good = fixture.enqueue(&b, None);
    executable(
        &fixture.trace.join("hook"),
        &format!(
            "#!/usr/bin/env bash\ncommand git cat-file -e HEAD:new.py 2>/dev/null || exit 0\ncommand git rev-parse HEAD >>\"$LAND_TRACE/captured\"\ncommand git reset -q --hard {}\necho refused >\"$LAND_TRACE/verdict-$1\"\n",
            fixture.base
        ),
    );
    assert!(fixture.serve().status.success());
    let captured = fixture.read("captured");
    assert_eq!(captured.lines().count(), 2, "the refused pair, then the Python lane alone");
    for candidate in captured.lines() {
        git(&fixture.repo, &["cat-file", "-e", &format!("{candidate}:new.py")]);
    }
    assert!(fixture.verdict(&bad).unwrap().starts_with("failed "), "the lane whose candidate moved HEAD is refused");
    assert_eq!(fixture.verdict(&good), Some(format!("landed {}", fixture.main())));
    assert_eq!(fixture.subjects(), ["change from b"]);
    assert!(git(&fixture.origin, &["ls-tree", "--name-only", "main"]).lines().all(|file| file != "new.py"), "the captured Python candidate was published");
    assert_eq!(fixture.read("pushes").lines().count(), 1);
}

#[test]
fn serve_transient_not_culprit() {
    let mut fixture = Fixture::new();
    let tickets: Vec<PathBuf> = ["a", "b", "c"].iter().map(|name| {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None)
    }).collect();
    fs::write(fixture.trace.join("verdict-1"), "refused https://github.com/o/r/actions/runs/1").unwrap();
    fs::write(fixture.trace.join("verdict-2"), "refused https://github.com/o/r/actions/runs/2").unwrap();
    fs::write(fixture.trace.join("exit-2"), "75").unwrap();
    assert_eq!(fixture.serve().status.code(), Some(75));
    assert_eq!(fixture.checks().len(), 4, "the refused candidate, then each lane alone");
    assert!(tickets.iter().all(|ticket| ticket.exists()), "an exit-75 check decided a lane");
    assert_eq!(fixture.main(), fixture.base);
    assert!(fixture.fleet().is_empty(), "{:?}", fixture.fleet());

    let racer = fixture.root.join("racer");
    git(&fixture.root, &["clone", "-q", fixture.origin.to_str().unwrap(), racer.to_str().unwrap()]);
    git(&racer, &["config", "user.name", "Racer"]);
    git(&racer, &["config", "user.email", "racer@example.invalid"]);
    fs::write(fixture.trace.join("verdict-5"), "refused https://github.com/o/r/actions/runs/5").unwrap();
    executable(
        &fixture.trace.join("hook"),
        &format!("#!/usr/bin/env bash\n[ \"$1\" = 5 ] || exit 0\ncd {}\necho race >race.txt\ncommand git add race.txt\ncommand git commit -qm 'main moves during the check'\ncommand git push -q origin main\n", racer.display()),
    );
    assert!(fixture.serve().status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 6, "a refusal on a moved base is rebuilt, not searched");
    assert_eq!(checks[5].1, git(&racer, &["rev-parse", "HEAD"]));
    assert_eq!(fixture.subjects(), ["main moves during the check", "change from a", "change from b", "change from c"]);
    let main = fixture.main();
    for ticket in &tickets {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
    }
    let mut fleet = fixture.fleet();
    fleet.sort();
    assert_eq!(fleet, ["a", "b", "c"].map(|name| format!("fleet send --from service:north-land session-lane-{name} landed lane-{name} as {main}")), "only landed messages go out");
}

fn persistent_isolates(verdict: &str, exit: &str, class: &str) {
    let mut fixture = Fixture::new();
    let tickets: Vec<PathBuf> = ["a", "b", "c"].iter().map(|name| {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None)
    }).collect();
    fs::write(fixture.trace.join("poison"), format!("b.txt\n{verdict}\n{exit}\n")).unwrap();
    for _ in 0..2 {
        assert_eq!(fixture.serve().status.code(), Some(75));
    }
    assert_eq!(fixture.checks().len(), 2);
    assert!(tickets.iter().all(|ticket| ticket.exists()), "a {class} decided a lane before the third try");
    assert_eq!(fixture.serve().status.code(), Some(75));
    let checks = fixture.checks();
    assert_eq!(checks.len(), 6, "the third {class} searches each lane alone");
    assert_eq!(tree(&fixture, &checks[0].0), tree(&fixture, &checks[2].0));
    for alone in &checks[3..] {
        assert_eq!(git(&fixture.repo, &["rev-list", "--count", &format!("{}..{}", fixture.base, alone.0)]), "1", "each lane is checked on its own");
    }
    assert!(tickets.iter().all(|ticket| ticket.exists()), "the search decided a lane that only ended without a verdict");
    assert_eq!(fixture.main(), fixture.base);
    let mut rounds = 0;
    while fixture.serve().status.code() == Some(75) {
        rounds += 1;
        assert!(rounds < 12, "the queue never settled");
    }
    let failure = fixture.verdict(&tickets[1]).unwrap();
    assert!(failure.starts_with(&format!("failed persistent {class}: ")) && failure.ends_with("-candidate.log"), "{failure}");
    let main = fixture.main();
    for ticket in [&tickets[0], &tickets[2]] {
        assert!(fixture.verdict(ticket).is_some_and(|verdict| verdict.starts_with("landed ")));
    }
    assert_eq!(fixture.verdict(&tickets[2]), Some(format!("landed {main}")));
    assert_eq!(fixture.subjects(), ["change from a", "change from c"]);
    let fleet = fixture.fleet();
    assert!(fleet.iter().any(|call| call.starts_with("fleet send --from service:north-land session-lane-b ") && call.contains(&format!("did not land: {failure}"))), "{fleet:?}");
    assert!(fleet.iter().filter(|call| call.contains("did not land")).count() == 1, "{fleet:?}");
}

#[test]
fn serve_persistent_transient_isolates() {
    persistent_isolates("green", "75", "transient");
}

#[test]
fn serve_persistent_deferred_isolates() {
    persistent_isolates("deferred DEFER_QUEUED", "0", "deferred");
}

#[test]
fn serve_defer() {
    let mut fixture = Fixture::new();
    let tickets: Vec<PathBuf> = ["a", "b", "c"].iter().map(|name| {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None)
    }).collect();
    fs::write(fixture.trace.join("verdict-1"), "deferred DEFER_QUEUED").unwrap();
    assert_eq!(fixture.serve().status.code(), Some(75));
    assert!(tickets.iter().all(|ticket| ticket.exists()));
    fs::write(fixture.trace.join("verdict-2"), "refused https://github.com/o/r/actions/runs/2").unwrap();
    fs::write(fixture.trace.join("verdict-3"), "deferred DEFER_QUEUED").unwrap();
    assert_eq!(fixture.serve().status.code(), Some(75));
    assert!(tickets.iter().all(|ticket| ticket.exists()), "a DEFER during the halving ejected a lane");
    assert_eq!(fixture.main(), fixture.base);
    assert_eq!(fixture.read("pushes"), "");
    assert!(fixture.serve().status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 6, "the deferred candidate, the refused candidate, three lanes alone, then the candidate again");
    assert_eq!((tree(&fixture, &checks[5].0), &checks[5].1), (tree(&fixture, &checks[0].0), &checks[0].1), "the deferred candidate is retried as it was");
    let main = fixture.main();
    assert_eq!(main, checks[5].0);
    for ticket in &tickets {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
    }
}

#[test]
fn serve_red_main_lands_fixes_first() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let fix = fixture.lane("fix", "fix.txt", "fix");
    let held = fixture.enqueue(&a, None);
    let fixed = fixture.enqueue(&fix, Some(RedFix::Marker));
    fs::write(fixture.trace.join("verdict-1"), "red-main abc").unwrap();
    fs::write(fixture.trace.join("verdict-3"), "red-main abc").unwrap();
    assert_eq!(fixture.serve().status.code(), Some(75));
    assert_eq!(fixture.subjects(), ["change from fix"]);
    assert_eq!(fixture.verdict(&fixed), Some(format!("landed {}", fixture.main())));
    assert!(held.exists(), "red main decided the held lane");
    let checks = fixture.checks();
    assert_eq!(checks.len(), 3);
    assert_eq!(git(&fixture.repo, &["ls-tree", "--name-only", &checks[1].0]), "app.txt\nfix.txt", "the red-fix lane is checked alone");
}

#[test]
fn serve_dropped_commit() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let dropped = git(&a, &["rev-parse", "HEAD"]);
    fixture.commit(&a, "b.txt", "second", "second change from a");
    let ticket = fixture.enqueue(&a, None);
    fs::write(fixture.repo.join("a.txt"), "a").unwrap();
    fixture.commit(&fixture.repo, "other.txt", "other", "a.txt and more from elsewhere");
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    let main = fixture.main();
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.main(), main, "the refused lane moved main");
    let verdict = fixture.verdict(&ticket).unwrap();
    let log = verdict.strip_prefix("failed ").unwrap_or_else(|| panic!("{verdict}"));
    let text = fs::read_to_string(log).unwrap();
    assert!(text.contains("lane commits missing") && text.contains(&dropped), "{text}");
    assert!(fixture.checks().is_empty());
    let fleet = fixture.fleet();
    assert!(fleet.len() == 1 && fleet[0].contains(log), "{fleet:?}");
}

#[test]
fn serve_context_only() {
    let mut fixture = Fixture::new();
    let twenty: String = (1..=20).map(|line| format!("{line}\n")).collect();
    fixture.commit(&fixture.repo, "shared.txt", &twenty, "twenty shared lines");
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    let a = fixture.lane("a", "a.txt", "a");
    let edit = |text: &str, lines: &[(usize, &str)]| -> String {
        text.lines().enumerate().map(|(index, line)| format!("{}\n", lines.iter().find(|(at, _)| *at == index + 1).map_or(line, |(_, new)| *new))).collect()
    };
    fixture.commit(&a, "shared.txt", &edit(&twenty, &[(2, "two"), (18, "eighteen")]), "a edits two shared lines");
    let ticket = fixture.enqueue(&a, None);
    fs::write(fixture.repo.join("other.txt"), "other\n").unwrap();
    fixture.commit(&fixture.repo, "shared.txt", &edit(&twenty, &[(2, "two")]), "main edits the first of them");
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())), "a lane whose diff changed only in context was refused");
    assert!(git(&fixture.origin, &["show", "main:shared.txt"]).lines().any(|line| line == "eighteen"));
    git(&fixture.origin, &["cat-file", "-e", "main:a.txt"]);
}
