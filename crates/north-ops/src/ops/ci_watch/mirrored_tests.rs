use north_github_mirror::store::{self, FRESH_S, Mirror, key};
use serde_json::{Value, json};

use crate::ops::ci_watch::{diagnose, notify};

const AT: f64 = 1000.0;
const REPO: &str = "tompassarelli/firn";

fn mirror_with(comments: usize) -> (tempfile::TempDir, Mirror) {
    let root = tempfile::tempdir().unwrap();
    let mirror = Mirror::at(root.path().join("github-mirror.db"));
    let bodies: Vec<Value> = (0..comments).map(|n| json!({"body": format!("comment {n}")})).collect();
    let issues = json!([
        {"number": 7, "title": "main is red", "body": "- First failing commit: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "comments": {"nodes": bodies}},
        {"number": 8, "title": "something else", "body": "", "comments": {"nodes": []}}
    ]);
    let runs = json!({"workflow_runs": [{"id": 55, "name": "ci", "conclusion": "failure", "status": "completed", "event": "push", "head_sha": "abc", "html_url": "https://github.com/x/runs/55", "updated_at": "2026-11-06T05:00:00Z"}]});
    mirror.write(&key("firn", store::OPEN_ISSUES), &issues, None, AT).unwrap();
    mirror.write(&key("firn", store::MAIN_RUNS), &runs, None, AT).unwrap();
    (root, mirror)
}

#[test]
fn a_fresh_mirror_answers_the_ci_watch_reads_with_no_github_call() {
    let (_root, mirror) = mirror_with(2);
    let now = AT + FRESH_S;
    let issues = diagnose::issues_via(&mirror, now, REPO, || panic!("GitHub called")).unwrap();
    assert_eq!(issues.iter().map(|issue| issue.number).collect::<Vec<_>>(), [7]);
    assert_eq!(diagnose::comments_via(&mirror, now, REPO, 7, || panic!("GitHub called")).unwrap(), ["comment 0", "comment 1"]);
    let runs = diagnose::runs_via(&mirror, now, REPO, || panic!("GitHub called")).unwrap();
    assert_eq!((runs[0].database_id, runs[0].workflow_name.as_str(), runs[0].conclusion.as_deref(), runs[0].url.as_str()), (55, "ci", Some("failure"), "https://github.com/x/runs/55"));
    let red = notify::issues_via(&mirror, now, REPO, || panic!("GitHub called")).unwrap();
    assert_eq!((red[0].number, red[0].url.as_str()), (7, "https://github.com/tompassarelli/firn/issues/7"));
    assert!(red[0].body.contains("First failing commit"));
    assert!(notify::run_finished_via(&mirror, now, REPO, "55", || panic!("GitHub called")).is_some());
}

