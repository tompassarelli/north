use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use north_landing::land::queue;

use super::{Fixture, executable, git};

const FAIL_HOOK: &str = "#!/usr/bin/env bash\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo 'refused https://github.com/o/r/actions/runs/5151' >\"$LAND_TRACE/verdict-$1\"\n";

fn shared(fixture: &Fixture) {
    fs::write(fixture.repo.join("shared.txt"), "line one\nline two\n").unwrap();
    git(&fixture.repo, &["add", "shared.txt"]);
    git(&fixture.repo, &["commit", "-qm", "add shared.txt"]);
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
}

fn done_verdicts(fixture: &Fixture, branch: &str) -> Vec<String> {
    let suffix = format!("-{branch}.json");
    let mut names: Vec<PathBuf> = fs::read_dir(fixture.queue().join("done")).unwrap().filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.to_string_lossy().ends_with(&suffix)).collect();
    names.sort();
    names.iter().filter_map(|path| queue::read(path).ok()?.verdict).collect()
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn batch_mate_defers() {
    let mut fixture = Fixture::new();
    shared(&fixture);
    let a = fixture.lane("a", "shared.txt", "line one from a\nline two\n");
    fixture.commit(&a, "fail.txt", "fail\n", "fail marker from a");
    let b = fixture.lane("b", "shared.txt", "line one from b\nline two\n");
    let c = fixture.lane("c", "other.txt", "other\n");
    let tickets = [fixture.enqueue(&a, None), fixture.enqueue(&b, None), fixture.enqueue(&c, None)];
    executable(&fixture.trace.join("hook"), FAIL_HOOK);
    let output = fixture.serve();
    assert!(output.status.success());
    assert!(stdout(&output).contains("deferred: lane-b"), "{}", stdout(&output));
    assert_eq!(fixture.verdict(&tickets[0]).as_deref(), Some("failed https://github.com/o/r/actions/runs/5151"));
    assert!(fixture.verdict(&tickets[2]).is_some_and(|verdict| verdict.starts_with("landed ")), "{:?}", fixture.verdict(&tickets[2]));
    assert_eq!(fixture.verdict(&tickets[1]), Some(format!("landed {}", fixture.main())));
    assert_eq!(done_verdicts(&fixture, "lane-b"), [format!("landed {}", fixture.main())], "lane-b got a conflict verdict");
    assert_eq!(git(&fixture.origin, &["show", "main:shared.txt"]), "line one from b\nline two");
    git(&fixture.origin, &["cat-file", "-e", "main:other.txt"]);
}

#[test]
fn batch_mate_gets_one_conflict_verdict_on_the_main_that_holds_the_lane_ahead() {
    let mut fixture = Fixture::new();
    shared(&fixture);
    let a = fixture.lane("a", "shared.txt", "line one from a\nline two\n");
    let b = fixture.lane("b", "shared.txt", "line one from b\nline two\n");
    let tickets = [fixture.enqueue(&a, None), fixture.enqueue(&b, None)];
    let output = fixture.serve();
    assert!(output.status.success());
    assert!(stdout(&output).contains("deferred: lane-b"), "{}", stdout(&output));
    let main = fixture.main();
    assert_eq!(fixture.verdict(&tickets[0]), Some(format!("landed {main}")));
    assert_eq!(done_verdicts(&fixture, "lane-b"), ["conflict shared.txt"]);
    assert_eq!(git(&fixture.origin, &["show", "main:shared.txt"]), "line one from a\nline two");
    let told: Vec<String> = fixture.fleet().into_iter().filter(|line| line.starts_with("fleet send --from service:north-land session-lane-b ")).collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("shared.txt"), "{told:?}");
}

#[test]
fn sent_back_keeps_position() {
    let mut fixture = Fixture::new();
    shared(&fixture);
    let s = fixture.lane("s", "shared.txt", "line one from s\nline two\n");
    fs::write(fixture.repo.join("shared.txt"), "line one from main\nline two\n").unwrap();
    git(&fixture.repo, &["commit", "-qam", "main changes shared.txt"]);
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    let first = fixture.enqueue(&s, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.verdict(&first).as_deref(), Some("conflict shared.txt"));
    let x = fixture.lane("x", "x.txt", "x\n");
    let y = fixture.lane("y", "y.txt", "y\n");
    fixture.enqueue(&x, None);
    fixture.enqueue(&y, None);
    git(&s, &["fetch", "-q", "origin"]);
    git(&s, &["reset", "-q", "--hard", "origin/main"]);
    fixture.commit(&s, "shared.txt", "line one from s\nline two\n", "change from s, rebased");
    let again = fixture.enqueue(&s, None);
    let order: Vec<String> = queue::pending(&fixture.queue()).unwrap().into_iter().map(|(_, ticket)| ticket.branch).collect();
    assert_eq!(order, ["lane-s", "lane-x", "lane-y"]);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.verdict(&again), Some(format!("landed {}", fixture.main())));
    let subjects = fixture.subjects();
    assert_eq!(subjects[subjects.len() - 3..], ["change from s, rebased", "change from x", "change from y"], "{subjects:?}");
}

fn age(path: &Path, by: Duration) {
    fs::File::options().write(true).open(path).unwrap().set_modified(SystemTime::now() - by).unwrap();
}

#[test]
fn a_send_back_25_hours_old_keeps_no_position() {
    let mut fixture = Fixture::new();
    shared(&fixture);
    let s = fixture.lane("s", "shared.txt", "line one from s\nline two\n");
    let first = fixture.enqueue(&s, None);
    let done = queue::decide(&first, queue::read(&first).unwrap(), "conflict shared.txt".to_owned()).unwrap();
    age(&done, Duration::from_secs(25 * 60 * 60));
    let x = fixture.lane("x", "x.txt", "x\n");
    fixture.enqueue(&x, None);
    fixture.enqueue(&s, None);
    let order: Vec<String> = queue::pending(&fixture.queue()).unwrap().into_iter().map(|(_, ticket)| ticket.branch).collect();
    assert_eq!(order, ["lane-x", "lane-s"]);
}
