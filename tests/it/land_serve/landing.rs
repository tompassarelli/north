use std::fs;
use std::path::PathBuf;

use north_landing::land::queue::{self, RedFix};

use super::{Fixture, executable, git};

#[test]
fn serve_train() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = ["a", "b", "c"].iter().map(|name| fixture.lane(name, &format!("{name}.txt"), name)).collect();
    let tickets: Vec<PathBuf> = lanes.iter().map(|lane| fixture.enqueue(lane, None)).collect();
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c"]);
    assert_eq!(fixture.checks().len(), 1, "the candidate is checked once");
    let main = fixture.main();
    for ticket in &tickets {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
        assert!(!ticket.exists());
    }
}

#[test]
fn serve_several_commits_lands_each_commit_after_one_check() {
    let mut fixture = Fixture::new();
    let split = fixture.lane("split", "one.txt", "one");
    fixture.commit(&split, "two.txt", "two", "second change from split");
    fixture.commit(&split, "three.txt", "three", "third change from split");
    let (path, mut ticket) = fixture.ticket(&split, None);
    ticket.several_commits = Some("each commit reverts on its own".to_owned());
    queue::write(&path, &ticket).unwrap();
    let after = fixture.lane("after", "after.txt", "after");
    fixture.enqueue(&after, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from split", "second change from split", "third change from split", "change from after"]);
    assert_eq!(fixture.checks().len(), 1, "the candidate is checked once");
    let done = queue::read(&path.parent().unwrap().join("done").join(path.file_name().unwrap())).unwrap();
    assert_eq!(done.verdict, Some(format!("landed {}", fixture.main())));
    assert_eq!(done.several_commits.as_deref(), Some("each commit reverts on its own"));
}

#[test]
fn serve_red_fix_first() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let fix = fixture.lane("fix", "fix.txt", "fix");
    fixture.enqueue(&a, None);
    fixture.enqueue(&fix, Some(RedFix::Marker));
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from fix", "change from a"]);
}

#[test]
fn serve_reenqueue_replaces() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "first");
    let older = fixture.enqueue(&a, None);
    git(&a, &["reset", "-q", "--hard", "HEAD~1"]);
    fixture.commit(&a, "a.txt", "second", "change from a, again");
    let newer = fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from a, again"]);
    assert_eq!(git(&fixture.origin, &["show", "main:a.txt"]), "second");
    assert!(fixture.verdict(&older).unwrap().starts_with("replaced"));
    assert_eq!(fixture.verdict(&newer), Some(format!("landed {}", fixture.main())));
}

#[test]
fn serve_cas_lost() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    let racer = fixture.root.join("racer");
    git(&fixture.root, &["clone", "-q", fixture.origin.to_str().unwrap(), racer.to_str().unwrap()]);
    git(&racer, &["config", "user.name", "Racer"]);
    git(&racer, &["config", "user.email", "racer@example.invalid"]);
    executable(
        &fixture.trace.join("hook"),
        &format!("#!/usr/bin/env bash\n[ \"$1\" = 1 ] || exit 0\ncd {}\necho race >race.txt\ncommand git add race.txt\ncommand git commit -qm 'per-lane landing under the lander'\ncommand git push -q origin main\n", racer.display()),
    );
    assert!(fixture.serve().status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 2, "a lost lease rebuilds and checks again");
    let race = git(&racer, &["rev-parse", "HEAD"]);
    assert_eq!(checks[1].1, race, "the rebuilt candidate is on the new main");
    assert_eq!(fixture.main(), checks[1].0);
    assert_eq!(fixture.subjects(), ["per-lane landing under the lander", "change from a"]);
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())));
}

#[test]
fn serve_pushes_checked_tree() {
    let mut fixture = Fixture::new();
    for name in ["a", "b"] {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None);
    }
    assert!(fixture.serve().status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 1);
    assert_eq!(fixture.main(), checks[0].0, "main is the very commit that was checked");
    assert_eq!(checks[0].1, fixture.base);
}

#[test]
fn serve_candidate_head_unchanged() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    fs::write(a.join("scratch.txt"), "untracked\n").unwrap();
    fs::write(a.join("app.txt"), "edited but not committed\n").unwrap();
    let before = (git(&a, &["rev-parse", "HEAD"]), git(&a, &["symbolic-ref", "HEAD"]), git(&a, &["status", "--porcelain"]));
    fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    assert_eq!((git(&a, &["rev-parse", "HEAD"]), git(&a, &["symbolic-ref", "HEAD"]), git(&a, &["status", "--porcelain"])), before);
    assert_eq!(fixture.subjects(), ["change from a"]);

    let b = fixture.lane("b", "b.txt", "b");
    let ticket = fixture.enqueue(&b, None);
    let main = fixture.main();
    let lander = fixture.root.join("proj/worktrees/lander");
    executable(&fixture.trace.join("hook"), &format!("#!/usr/bin/env bash\ncommand git -C {} commit -q --allow-empty -m moved\n", lander.display()));
    let output = fixture.serve();
    assert_eq!(output.status.code(), Some(75));
    assert!(String::from_utf8_lossy(&output.stdout).contains("moved while candidate"));
    assert_eq!(fixture.main(), main, "nothing is pushed when the lander worktree moved during the check");
    assert!(ticket.exists(), "the lane stays queued");
}

#[test]
fn serve_gate_refusal() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    fs::write(fixture.trace.join("verdict"), "refused https://github.com/o/r/actions/runs/7").unwrap();
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.main(), fixture.base);
    assert_eq!(fixture.verdict(&ticket).as_deref(), Some("failed https://github.com/o/r/actions/runs/7"));
    assert_eq!(fixture.read("pushes"), "", "a refused candidate is not pushed");
}

fn refuse_pushes(fixture: &Fixture, message: &str) {
    executable(&fixture.origin.join("hooks/pre-receive"), &format!("#!/usr/bin/env bash\necho '{message}' >&2\nexit 1\n"));
}

#[test]
fn serve_archived_origin_refusal_is_terminal() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    refuse_pushes(&fixture, "This repository was archived so it is read-only.");
    let output = fixture.serve();
    assert!(output.status.success(), "a terminal refusal decides the ticket");
    assert_eq!(fixture.main(), fixture.base);
    assert_eq!(fixture.read("pushes").lines().count(), 1, "the push is attempted once");
    let verdict = fixture.verdict(&ticket).unwrap();
    assert!(verdict.starts_with("failed origin refused the push of ") && verdict.ends_with(": remote: This repository was archived so it is read-only."), "{verdict}");
    let fleet = fixture.fleet();
    assert!(fleet.len() == 1 && fleet[0].starts_with("fleet send --from service:north-land session-lane-a north land: lane-a ") && fleet[0].contains("This repository was archived so it is read-only."), "{fleet:?}");
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.checks().len(), 1, "no further candidate is checked for the refused ticket");
    assert_eq!(fixture.read("pushes").lines().count(), 1);
}

#[test]
fn serve_other_origin_refusal_waits_and_keeps_the_ticket() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    refuse_pushes(&fixture, "the server is having a moment");
    let output = fixture.serve();
    assert_eq!(output.status.code(), Some(75));
    assert!(String::from_utf8_lossy(&output.stdout).contains("waiting: origin refused the push of "), "{}", String::from_utf8_lossy(&output.stdout));
    assert!(ticket.exists(), "a refusal of unknown cause keeps the lane queued");
    assert_eq!(fixture.read("pushes").lines().count(), 1);
    assert!(fixture.fleet().is_empty(), "{:?}", fixture.fleet());
}
#[test]
fn serve_infrastructure_verdicts_keep_tickets() {
    for class in ["red-main abc", "transient fetch", "deferred DEFER_QUEUED"] {
        let mut fixture = Fixture::new();
        let a = fixture.lane("a", "a.txt", "a");
        let ticket = fixture.enqueue(&a, None);
        fs::write(fixture.trace.join("verdict"), class).unwrap();
        assert_eq!(fixture.serve().status.code(), Some(75));
        assert!(ticket.exists(), "{class} decided the lane");
        assert_eq!(fixture.main(), fixture.base);
    }
}

#[test]
fn serve_stale_revert() {
    let mut fixture = Fixture::new();
    fixture.commit(&fixture.repo, "app.txt", "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\n", "x");
    fixture.commit(&fixture.repo, "app.txt", "alpha changed by y\nbeta\ngamma\ndelta\nepsilon\nzeta\n", "y");
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    let main = fixture.main();
    let a = fixture.lane("a", "notes.txt", "lane work\n");
    git(&a, &["checkout", &fixture.base, "--", "app.txt"]);
    git(&a, &["commit", "-q", "--amend", "-am", "lane copies app.txt from an older ref"]);
    let ticket = fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.main(), main);
    let verdict = fixture.verdict(&ticket).unwrap();
    let log = verdict.strip_prefix("failed ").unwrap();
    assert!(fs::read_to_string(log).unwrap().contains("undoes recent commits"));
    assert!(fixture.checks().is_empty());
}

#[test]
fn serve_already_on_main() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    git(&fixture.repo, &["cherry-pick", &git(&a, &["rev-parse", "HEAD"])]);
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    let main = fixture.main();
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.main(), main);
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {main}")));
    assert!(fixture.checks().is_empty());
    assert_eq!(fixture.read("pushes"), "");
}

#[test]
fn serve_token_scoped() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    let env = fixture.read("env-1");
    assert!(env.contains("LAND_TRACE="));
    for name in ["GH_TOKEN=", "GITHUB_TOKEN=", "SOPS_AGE_KEY_FILE="] {
        assert!(!env.lines().any(|line| line.starts_with(name)), "the check saw {name}");
    }
    assert!(fixture.read("pushes").contains("token=push-token"), "the push step keeps its token");
}

#[test]
fn serve_check_cannot_reach_the_lander_pin() {
    let mut fixture = Fixture::new();
    let link = fixture.root.join("lander");
    std::os::unix::fs::symlink("gen-1", &link).unwrap();
    let a = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&a, None);
    let output = fixture.command().env("NORTH_RUNTIME_LINK", &link).output().unwrap();
    eprintln!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    let env = fixture.read("env-1");
    assert!(env.contains("LAND_TRACE="));
    assert!(!env.lines().any(|line| line.starts_with("NORTH_RUNTIME_LINK=")), "the check saw the lander's own runtime link, so a test stub it runs could repoint the pin");
}

#[test]
fn serve_all_serves_every_repository_without_sharing() {
    let north = Fixture::new();
    let mut south = Fixture::new();
    south.state = north.state.clone();
    let north_lane = north.lane("a", "a.txt", "a");
    let south_lane = south.lane("b", "b.txt", "b");
    for (fixture, lane) in [(&north, &north_lane), (&south, &south_lane)] {
        assert!(fixture.land_enqueue(lane, &fixture.state, "session").status.success());
    }
    assert_ne!(north.queue(), south.queue(), "each origin has its own queue directory");
    assert_eq!(queue::pending(&north.queue()).unwrap().len(), 1);
    assert_eq!(queue::pending(&south.queue()).unwrap().len(), 1);
    let output = north.command_with(["--all"]).output().unwrap();
    eprintln!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    assert_eq!(north.subjects(), ["change from a"]);
    assert_eq!(south.subjects(), ["change from b"]);
    for fixture in [&north, &south] {
        assert!(queue::pending(&fixture.queue()).unwrap().is_empty());
        let landed = fs::read_dir(fixture.queue().join("done")).unwrap().filter_map(Result::ok).filter_map(|entry| queue::read(&entry.path()).ok()).filter(|ticket| ticket.verdict.as_deref() == Some(&format!("landed {}", fixture.main()))).count();
        assert_eq!(landed, 1, "the repository's own ticket is done in its own queue");
        assert!(fixture.root.join("proj/worktrees/lander/.git").exists());
        assert!(!fixture.queue().join("lock").exists());
    }
    assert_ne!(north.root.join("proj/worktrees/lander"), south.root.join("proj/worktrees/lander"));
    let checked: Vec<String> = north.checks().into_iter().map(|(head, _)| head).collect();
    assert_eq!(checked.len(), 2, "each repository's candidate is checked once: {checked:?}");
    assert!(checked.contains(&north.main()) && checked.contains(&south.main()), "each candidate was checked in its own checkout and is what landed: {checked:?}");
}

#[test]
fn serve_reaps_a_previous_landers_remote_offload_runs_before_it_serves() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    let calls = fixture.read("north-calls");
    let owner = format!("capacity offload reap --owner north-land-{}", fixture.queue().file_name().unwrap().to_str().unwrap());
    let reap = calls.lines().position(|line| line == owner).expect(&calls);
    let first_check = fixture.read("checks");
    assert!(!first_check.is_empty());
    assert_eq!(reap, 0, "the reap is the first thing the lander asks of north: {calls}");
}

#[test]
fn serve_marks_its_checks_with_the_offload_owner_the_reaper_matches() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    let owner = format!("NORTH_OFFLOAD_OWNER=north-land-{}", fixture.queue().file_name().unwrap().to_str().unwrap());
    assert!(fixture.read("env-1").lines().any(|line| line == owner), "{}", fixture.read("env-1"));
}

#[test]
fn serve_github_writes() {
    let mut fixture = Fixture::new();
    for name in ["a", "b", "c"] {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None);
    }
    assert!(fixture.serve().status.success());
    let pushes = fixture.read("pushes");
    assert_eq!(pushes.lines().count(), 1, "{pushes}");
    assert!(pushes.contains(":refs/heads/main") && pushes.contains("--force-with-lease=refs/heads/main:"));
    let github = fixture.read("github");
    assert_eq!(github.lines().filter(|line| *line == "land-check ref push").count(), 1);
    assert_eq!(github.lines().filter(|line| *line == "dispatch").count(), 1);
    assert_eq!(github.lines().filter(|line| *line == "ref delete").count(), 1);
}
