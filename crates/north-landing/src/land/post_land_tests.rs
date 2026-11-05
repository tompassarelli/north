use std::cell::RefCell;
use std::path::Path;
use std::process::Command;

use super::*;

fn repository(root: &Path) -> String {
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let git = |arguments: &[&str]| assert!(Command::new("git").arg("-C").arg(&repo).args(arguments).status().unwrap().success());
    git(&["init", "-q", "-b", "main"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "landed"]);
    String::from_utf8(Command::new("git").arg("-C").arg(&repo).args(["rev-parse", "HEAD"]).output().unwrap().stdout).unwrap().trim().to_owned()
}

fn fixture(root: &Path, script: &str) -> (Landed, Run) {
    let head = repository(root);
    let landed = Landed { repo: root.join("repo"), repository: "tompassarelli/north".into(), base: head.clone(), head, lanes: vec!["lane-a".into(), "lane-b".into()] };
    let run = Run {
        runner: vec!["sh".into(), "-c".into(), script.into(), "sh".into()],
        worktree: root.join("post-land"),
        log: root.join("logs/post-land.log"),
        outcome: root.join("logs/post-land.outcome"),
        ledger: root.join("post-land.tsv"),
        offload_owner: "north-land-test".into(),
    };
    (landed, run)
}

#[test]
fn a_failing_after_check_records_red_main_on_the_landed_sha_and_alarms_the_proxy_once() {
    let root = tempfile::tempdir().unwrap();
    let (landed, run) = fixture(root.path(), "printf 'event\\tland check\\tfailed\\tcargo-tests.sh; log: x\\n' > \"$6\"; echo 'test parse_lines ... FAILED'; exit 1");
    let sent = RefCell::new(Vec::new());
    let verdict = settle(&landed, &run, |session, message| {
        sent.borrow_mut().push((session.to_owned(), message.to_owned()));
        true
    });
    assert_eq!(verdict, Verdict::RedMain("cargo-tests.sh; log: x".into()));
    let sent = sent.into_inner();
    assert_eq!(sent.len(), 1);
    let (session, message) = &sent[0];
    assert_eq!(session, "proxy");
    assert!(message.contains("cargo-tests.sh") && message.contains("lane-a, lane-b") && message.contains(&landed.head) && message.contains("parse_lines ... FAILED"), "{message}");
    let ledger = fs::read_to_string(&run.ledger).unwrap();
    assert!(ledger.contains(&format!("\t{}\tred-main\tcargo-tests.sh", landed.head)), "{ledger}");
    assert!(!run.worktree.exists());
}

#[test]
fn a_passing_after_set_records_green_and_alarms_nobody() {
    let root = tempfile::tempdir().unwrap();
    let (landed, run) = fixture(root.path(), "test \"$4\" = \"$(git rev-parse HEAD)\"");
    let verdict = settle(&landed, &run, |_, _| panic!("alarmed on green"));
    assert_eq!(verdict, Verdict::Green);
    assert!(fs::read_to_string(&run.ledger).unwrap().contains(&format!("\t{}\tgreen\t", landed.head)));
}
