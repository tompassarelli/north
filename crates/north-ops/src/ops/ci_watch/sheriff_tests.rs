use super::*;

fn step(name: &str, conclusion: &str) -> Step {
    Step { name: name.into(), number: 2, conclusion: Some(conclusion.into()) }
}

fn job(conclusion: &str, steps: Vec<Step>) -> Job {
    Job { id: 7, name: "test".into(), conclusion: Some(conclusion.into()), steps }
}

fn commit(sha: &str, parent: &str, message: &str, file: &str) -> Commit {
    Commit { sha: sha.into(), parents: vec![parent.into()], message: message.into(), files: vec![file.into()] }
}

fn recorded() -> Situation {
    Situation {
        attempt: 1,
        conclusion: Some("failure".into()),
        original: vec![job("failure", vec![step("Test", "failure")])],
        confirmed: Vec::new(),
        previous: Some(Previous { sha: "parent".into(), conclusion: Some("success".into()) }),
        commits: vec![commit("culprit", "parent", "land", "src/main.rs")],
        on_main: true,
        head: "culprit".into(),
        tip: "culprit".into(),
        successor: None,
    }
}

fn confirmation() -> Situation {
    Situation { attempt: 2, confirmed: recorded().original, ..recorded() }
}

fn only(change: impl Fn(&mut Commit)) -> Situation {
    let mut s = confirmation();
    change(&mut s.commits[0]);
    s
}

#[test]
fn github_workflow_run_confirmation_protocol() {
    assert_eq!(decide(&recorded()).kind, Kind::Confirm);
    let verdict = decide(&confirmation());
    assert_eq!((verdict.kind, verdict.shas.clone(), verdict.parent.as_deref(), verdict.job), (Kind::Request, vec!["culprit".to_owned()], Some("parent"), Some(7)));
    assert_eq!(
        revert_request("culprit", &verdict, &["test suite::x ... FAILED"], "https://example/runs/9"),
        "Main red at culprit\n\ntest: Test\ntest suite::x ... FAILED\n\nGreen-parent: parent\nMain-red-run: https://example/runs/9"
    );
    for (status, conclusion, said) in [("completed", Some("success"), "passed"), ("in_progress", None, "is still in_progress"), ("queued", None, "is still queued")] {
        let s = Situation {
            tip: "6b0baa426".into(),
            successor: Some(Successor { status: status.into(), conclusion: conclusion.map(str::to_owned) }),
            ..confirmation()
        };
        let verdict = decide(&s);
        assert_eq!(verdict.kind, Kind::Report);
        assert!(verdict.reason.contains(&format!("6b0baa426, whose run {said}")), "{}", verdict.reason);
    }
    let two = Situation {
        head: "tip".into(),
        tip: "tip".into(),
        commits: vec![commit("tip", "culprit", "second", "src/lib.rs"), commit("culprit", "parent", "land", "src/main.rs")],
        ..confirmation()
    };
    let verdict = decide(&two);
    assert_eq!((verdict.kind, verdict.shas, verdict.parent.as_deref()), (Kind::Request, vec!["tip".to_owned(), "culprit".to_owned()], Some("parent")));
    let cancelled = Some("cancelled".to_owned());
    let done = vec![job("cancelled", Vec::new())];
    for s in [
        Situation { conclusion: cancelled.clone(), original: done.clone(), ..recorded() },
        Situation { conclusion: cancelled.clone(), ..recorded() },
        Situation { conclusion: cancelled.clone(), confirmed: done, ..confirmation() },
        Situation { conclusion: cancelled, ..confirmation() },
    ] {
        assert_eq!(decide(&s).kind, Kind::None, "{s:?}");
    }
    for s in [
        Situation { tip: "fix-forward".into(), ..recorded() },
        Situation { tip: "fix-forward".into(), ..confirmation() },
        Situation { previous: Some(Previous { sha: "unrelated".into(), conclusion: Some("success".into()) }), ..confirmation() },
        Situation { previous: Some(Previous { sha: "parent".into(), conclusion: Some("failure".into()) }), ..confirmation() },
        Situation { attempt: 3, ..confirmation() },
        Situation { conclusion: Some("success".into()), confirmed: vec![job("success", Vec::new())], ..confirmation() },
        Situation { confirmed: vec![job("failure", vec![step("Other", "failure")])], ..confirmation() },
        only(|c| c.files = vec![".github/workflows/tests.yml".into()]),
        only(|c| c.message = "Revert bad landing\n\nSheriff-Reverts: prior".into()),
        Situation { on_main: false, ..confirmation() },
    ] {
        assert_eq!(decide(&s).kind, Kind::Report, "{s:?}");
    }
}

#[test]
fn a_new_issue_carries_one_done_when_box_and_appending_keeps_it() {
    let created = issue_body(None, "<!-- main-red:1:1 -->", "first failure");
    let appended = issue_body(Some(&created), "<!-- main-red:2:1 -->", "second failure");
    let boxes = |text: &str| text.lines().filter(|line| line.starts_with("- [ ] ") && line.contains("green")).count();
    assert!(created.contains("## Done when"));
    assert_eq!(boxes(&created), 1);
    assert!(appended.starts_with(&created));
    assert_eq!(boxes(&appended), 1);
    assert!(appended.contains("second failure"));
}

#[test]
fn a_long_running_issue_keeps_its_done_when_box_and_newest_failures_within_githubs_body_limit() {
    let mut text = issue_body(None, "<!-- main-red:0:1 -->", "first failure");
    for id in 1..400 {
        text = issue_body(Some(&text), &format!("<!-- main-red:{id}:1 -->"), &"failing test output line\n".repeat(20));
    }
    assert!(text.len() <= 60_000, "{}", text.len());
    assert!(text.starts_with("Main CI failures and confirmed culprit decisions.\n\n## Done when\n- [ ] "));
    assert!(text.contains("<!-- main-red:399:1 -->") && text.contains("<!-- main-red:398:1 -->"));
    assert!(!text.contains("<!-- main-red:0:1 -->"), "the oldest failure was kept past the limit");
}

#[test]
fn the_green_parent_is_the_newest_earlier_green_run_below_the_failing_commit_even_past_red_and_cancelled_runs() {
    let run = |id: u64, sha: &str, conclusion: &str| CiRun {
        id,
        workflow_id: 1,
        head_sha: sha.into(),
        head_branch: Some("main".into()),
        event: "push".into(),
        run_attempt: 1,
        conclusion: Some(conclusion.into()),
        html_url: String::new(),
    };
    let history = [run(1, "old", "success"), run(2, "green", "success"), run(3, "culprit", "failure"), run(4, "cancelled", "cancelled"), run(5, "side", "success"), run(6, "head", "failure"), run(7, "later", "success")];
    let below_head = |sha: &str| sha != "side" && sha != "later";
    assert_eq!(green_parent(&history, 6, below_head).map(|r| r.head_sha.as_str()), Some("green"));
    assert!(green_parent(&history, 2, |_| false).is_none());
}
