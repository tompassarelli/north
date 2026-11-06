use super::*;
use crate::read::{self, Seen};
use serde_json::json;
use std::cell::{Cell, RefCell};

struct Fake {
    issues_edited: Cell<bool>,
    issue_closed: Cell<bool>,
    graphql_fails: Cell<bool>,
    graphql_reads: Cell<u32>,
    closed_reads: Cell<u32>,
    branch_reads: Cell<u32>,
    sent_etags: RefCell<Vec<Option<String>>>,
}

impl Fake {
    fn new() -> Self {
        Self {
            issues_edited: Cell::new(false),
            issue_closed: Cell::new(false),
            graphql_fails: Cell::new(false),
            graphql_reads: Cell::new(0),
            closed_reads: Cell::new(0),
            branch_reads: Cell::new(0),
            sent_etags: RefCell::new(Vec::new()),
        }
    }
}

impl GitHub for Fake {
    fn get(&self, endpoint: &str, etag: Option<&str>) -> Result<Answer, String> {
        self.sent_etags.borrow_mut().push(etag.map(str::to_owned));
        let current = if endpoint.contains("/issues?") { if self.issues_edited.get() { "\"p2\"" } else { "\"p1\"" } } else { "\"r1\"" };
        if etag == Some(current) {
            return Ok(Answer::Unchanged);
        }
        Ok(Answer::Changed { body: json!([{"endpoint": endpoint, "etag": current}]), etag: Some(current.to_owned()) })
    }

    fn open_issues(&self, _repo: &str) -> Result<Vec<Value>, String> {
        self.graphql_reads.set(self.graphql_reads.get() + 1);
        if self.graphql_fails.get() {
            return Err("graphql down".into());
        }
        let open = if self.issue_closed.get() { vec![] } else { vec![json!({"number": 638, "title": "a local GitHub mirror", "edited": self.issues_edited.get()})] };
        Ok([open, vec![json!({"number": 9})]].concat())
    }

    fn closed_issues(&self, _repo: &str) -> Result<Vec<Value>, String> {
        self.closed_reads.set(self.closed_reads.get() + 1);
        Ok(if self.issue_closed.get() { vec![json!(7), json!(638)] } else { vec![json!(7)] })
    }

    fn branches(&self, _repo: &str) -> Result<Vec<Value>, String> {
        self.branch_reads.set(self.branch_reads.get() + 1);
        Ok(vec![json!({"name": "main", "target": {"oid": "abc"}})])
    }
}

fn outcomes(fake: &Fake, mirror: &Mirror, at: f64) -> Vec<Result<Fetch, String>> {
    repository(fake, mirror, "north", at).into_iter().map(|(_, result)| result).collect()
}

#[test]
fn an_unchanged_repository_costs_only_conditional_reads_and_an_edited_one_refetches_its_open_issues() {
    let root = tempfile::tempdir().unwrap();
    let (fake, mirror) = (Fake::new(), Mirror::at(root.path().join("github-mirror.db")));
    assert_eq!(outcomes(&fake, &mirror, 100.0), vec![Ok(Fetch::Refreshed); 12]);
    assert_eq!(fake.graphql_reads.get(), 1);
    fake.sent_etags.borrow_mut().clear();
    assert_eq!(outcomes(&fake, &mirror, 200.0), vec![Ok(Fetch::Unchanged); 12]);
    assert_eq!(fake.graphql_reads.get(), 1);
    assert_eq!(*fake.sent_etags.borrow(), [Some("\"p1\"")].into_iter().chain([Some("\"r1\""); 11]).map(|etag| etag.map(str::to_owned)).collect::<Vec<_>>());
    assert_eq!(mirror.fresh(&key("north", OPEN_ISSUES), 200.0 + store::FRESH_S), Some(json!([{"number": 638, "title": "a local GitHub mirror", "edited": false}, {"number": 9}])));
    fake.issues_edited.set(true);
    assert_eq!(outcomes(&fake, &mirror, 300.0)[0], Ok(Fetch::Refreshed));
    assert_eq!(mirror.read(&key("north", OPEN_ISSUES)).unwrap().body[0]["edited"], true);
}

#[test]
fn a_failed_issue_refetch_keeps_the_old_probe_so_the_next_sync_tries_again() {
    let root = tempfile::tempdir().unwrap();
    let (fake, mirror) = (Fake::new(), Mirror::at(root.path().join("github-mirror.db")));
    outcomes(&fake, &mirror, 100.0);
    fake.issues_edited.set(true);
    fake.graphql_fails.set(true);
    assert!(outcomes(&fake, &mirror, 200.0)[0].is_err());
    fake.graphql_fails.set(false);
    assert_eq!(outcomes(&fake, &mirror, 300.0)[0], Ok(Fetch::Refreshed));
    assert_eq!(fake.graphql_reads.get(), 3);
}

#[test]
fn gh_include_output_reads_as_unchanged_on_304_and_as_the_body_and_etag_on_200() {
    assert_eq!(response("HTTP/2.0 304 Not Modified\r\nEtag: W/\"abc\"\r\n\r\n"), Ok(Answer::Unchanged));
    let changed = "HTTP/2.0 200 OK\r\nContent-Type: application/json\r\nEtag: W/\"abc\"\r\nX-Ratelimit-Remaining: 4999\r\n\r\n[{\"id\":1}]";
    assert_eq!(response(changed), Ok(Answer::Changed { body: json!([{"id": 1}]), etag: Some("W/\"abc\"".into()) }));
    assert_eq!(response("HTTP/2.0 403 Forbidden\r\n\r\n{}"), Err("HTTP 403".into()));
}

#[test]
fn a_fresh_mirror_answers_branches_tags_releases_and_issue_states_without_calling_github() {
    let root = tempfile::tempdir().unwrap();
    let (fake, mirror) = (Fake::new(), Mirror::at(root.path().join("github-mirror.db")));
    outcomes(&fake, &mirror, 100.0);
    let at = 100.0 + store::FRESH_S;
    let never = || -> Result<Vec<Value>, String> { panic!("a mirror hit must not call GitHub") };
    assert_eq!(read::listed(&mirror, at, "north", store::BRANCHES, never).unwrap(), vec![json!({"name": "main", "target": {"oid": "abc"}})]);
    assert_eq!(read::listed(&mirror, at, "north", store::TAGS, never).unwrap().len(), 1);
    assert_eq!(read::listed(&mirror, at, "north", store::RELEASES, never).unwrap().len(), 1);
    assert_eq!(read::issue_state(&mirror, at, "north", 638), Some(Seen::Open));
    assert_eq!(read::issue_state(&mirror, at, "north", 7), Some(Seen::Closed));
    assert_eq!(read::issue_state(&mirror, at, "north", 5), None);
    assert_eq!(read::issue_state(&mirror, at + 1.0, "north", 638), None);
    assert_eq!(read::listed(&mirror, at + 1.0, "north", store::BRANCHES, || Ok(vec![json!("live")])).unwrap(), vec![json!("live")]);
    assert_eq!(read::listed(&mirror, at, "padbridge", store::BRANCHES, || Ok(vec![json!("live")])).unwrap(), vec![json!("live")]);
    assert_eq!(read::listed(&mirror, at, "firn", store::BRANCHES, || Ok(vec![json!("live")])).unwrap(), vec![json!("live")]);
    let never = || -> Result<Value, String> { panic!("a mirror hit must not call GitHub") };
    assert_eq!(read::pages(&mirror, at, "north", store::OPEN_PULLS, never).unwrap().as_array().map(Vec::len), Some(1));
    assert_eq!(read::pages(&mirror, at, "north", &store::runs_kind("queued"), never).unwrap().as_array().map(Vec::len), Some(1));
    mirror.write(&key("north", &store::runs_kind("waiting")), &json!({"total_count": 150, "workflow_runs": [{"head_branch": "a"}]}), None, 100.0).unwrap();
    assert_eq!(read::pages(&mirror, at, "north", &store::runs_kind("waiting"), || Ok(json!("live"))).unwrap(), json!("live"), "a run list GitHub holds more of than one page is read from GitHub");
    assert_eq!(read::pages(&mirror, at + 1.0, "north", store::OPEN_PULLS, || Ok(json!("live"))).unwrap(), json!("live"));
}

#[test]
fn closed_issue_numbers_are_refetched_only_when_the_open_set_changes_and_stay_fresh_otherwise() {
    let root = tempfile::tempdir().unwrap();
    let (fake, mirror) = (Fake::new(), Mirror::at(root.path().join("github-mirror.db")));
    outcomes(&fake, &mirror, 100.0);
    fake.issues_edited.set(true);
    outcomes(&fake, &mirror, 200.0);
    assert_eq!((fake.graphql_reads.get(), fake.closed_reads.get()), (2, 1));
    outcomes(&fake, &mirror, 300.0);
    assert_eq!(read::issue_state(&mirror, 300.0, "north", 7), Some(Seen::Closed));
    fake.issue_closed.set(true);
    fake.issues_edited.set(false);
    outcomes(&fake, &mirror, 400.0);
    assert_eq!(fake.closed_reads.get(), 2);
    assert_eq!(read::issue_state(&mirror, 400.0, "north", 638), Some(Seen::Closed));
    assert_eq!(fake.branch_reads.get(), 1);
}

