use super::github::{Upstream, read_open_issues};
use super::{IssueRef, Value};
use north_github_mirror::store::{FRESH_S, Mirror, OPEN_ISSUES, key};
use serde_json::json;

struct Unreachable;

impl Upstream for Unreachable {
    fn open_issue_nodes(&self, repo: &str) -> Result<Vec<Value>, String> {
        panic!("dispatch asked GitHub for {repo}'s open issues while the mirror held them")
    }

    fn still_open(&self, reference: &IssueRef) -> Option<bool> {
        panic!("dispatch asked GitHub whether {}#{} is open while the mirror held it", reference.repo, reference.number)
    }
}

struct Answers;

impl Upstream for Answers {
    fn open_issue_nodes(&self, _repo: &str) -> Result<Vec<Value>, String> {
        Ok(vec![json!({"number": 7, "title": "from GitHub", "body": ""})])
    }

    fn still_open(&self, _reference: &IssueRef) -> Option<bool> {
        Some(true)
    }
}

fn mirror_holding(root: &std::path::Path, at: f64) -> Mirror {
    let mirror = Mirror::at(root.join("github-mirror.db"));
    let north = json!([{"number": 638, "title": "a local GitHub mirror", "body": "Blocked by wisp#109 and wisp#110", "createdAt": "2026-11-05T00:00:00Z", "labels": {"nodes": [{"name": "tooling"}]}}]);
    mirror.write(&key("north", OPEN_ISSUES), &north, None, at).unwrap();
    mirror.write(&key("wisp", OPEN_ISSUES), &json!([{"number": 109, "title": "light", "body": ""}]), None, at).unwrap();
    mirror
}

#[test]
fn dispatch_reads_open_issues_and_their_blockers_from_a_fresh_mirror_without_calling_github() {
    let root = tempfile::tempdir().unwrap();
    let mirror = mirror_holding(root.path(), 1000.0);
    let mut notes = Vec::new();
    let (issues, open) = read_open_issues(&["north".to_owned()], &mirror, 1000.0 + FRESH_S, &Unreachable, &mut notes);
    assert_eq!(issues.iter().map(|issue| (issue.number, issue.labels.clone())).collect::<Vec<_>>(), vec![(638, vec!["tooling".to_owned()])]);
    assert_eq!(open.into_iter().collect::<Vec<_>>(), vec![IssueRef::new("north", 638), IssueRef::new("wisp", 109)]);
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn dispatch_falls_back_to_github_and_says_so_when_the_mirror_copy_is_stale() {
    let root = tempfile::tempdir().unwrap();
    let mirror = mirror_holding(root.path(), 1000.0);
    let mut notes = Vec::new();
    let (issues, _) = read_open_issues(&["north".to_owned()], &mirror, 1001.0 + FRESH_S, &Answers, &mut notes);
    assert_eq!(issues.iter().map(|issue| issue.number).collect::<Vec<_>>(), vec![7]);
    assert!(notes.iter().any(|note| note.starts_with("north issues read from GitHub")), "{notes:?}");
}
