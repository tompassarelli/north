use super::*;

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

#[test]
fn the_sheriff_closes_main_is_red_once_cargo_and_harness_pass_on_a_commit_after_the_culprits() {
    let run = |sha: &str, conclusion: &str| GateRun { head_sha: sha.into(), conclusion: Some(conclusion.into()), html_url: format!("run-{sha}") };
    let body = issue_body(None, "<!-- main-red:1:1 -->", "Culprit range: aaa, bbb\nFailing step: x");
    let body = issue_body(Some(&body), "<!-- main-red:2:1 -->", "Culprit range: ccc, ddd\nFailing step: y");
    let culprits = newest_culprits(&body);
    assert_eq!(culprits, ["ccc", "ddd"]);
    let after = |culprit: &str, head: &str| head == "later" && culprit != "later";
    assert_eq!(repaired_by(&culprits, &[run("later", "success"), run("later", "success")], after).map(|links| links.len()), Some(2));
    assert!(repaired_by(&culprits, &[run("later", "success"), run("later", "failure")], after).is_none());
    assert!(repaired_by(&culprits, &[run("later", "success"), run("ddd", "success")], after).is_none());
    assert!(repaired_by(&culprits, &[run("later", "success")], after).is_none());
}
