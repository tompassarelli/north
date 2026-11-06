use super::*;
use crate::audit;
use crate::read::{self, Seen};
use crate::store::BRANCHES_PROBE;
use serde_json::json;
use std::cell::{Cell, RefCell};

struct Fake {
    issues_edited: Cell<bool>,
    issue_closed: Cell<bool>,
    graphql_fails: Cell<bool>,
    graphql_reads: Cell<u32>,
    closed_reads: Cell<u32>,
    branch_reads: Cell<u32>,
    compare_reads: RefCell<Vec<Vec<String>>>,
    pages_reads: Cell<u32>,
    list_reads: Cell<u32>,
    main_moved: Cell<bool>,
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
            compare_reads: RefCell::new(Vec::new()),
            pages_reads: Cell::new(0),
            list_reads: Cell::new(0),
            main_moved: Cell::new(false),
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
        let main = if self.main_moved.get() { "def" } else { "abc" };
        Ok(vec![json!({"name": "main", "target": {"oid": main}}), json!({"name": "topic", "target": {"oid": "t1"}}), json!({"name": "landed", "target": {"oid": "l1"}})])
    }

    fn pages(&self, _repo: &str) -> Result<Value, String> {
        self.pages_reads.set(self.pages_reads.get() + 1);
        Ok(Value::Null)
    }

    fn repo_list(&self) -> Result<Value, String> {
        self.list_reads.set(self.list_reads.get() + 1);
        Ok(json!([{"name": "north"}]))
    }

    fn compares(&self, _repo: &str, branches: &[String]) -> Result<Vec<Value>, String> {
        self.compare_reads.borrow_mut().push(branches.to_vec());
        Ok(branches.iter().map(|branch| if branch == "landed" { json!({"ahead_by": 0, "messages": ""}) } else { json!({"ahead_by": 2, "messages": "Fix a thing (north#638)\n"}) }).collect())
    }
}

fn outcomes(fake: &Fake, mirror: &Mirror, at: f64) -> Vec<Result<Fetch, String>> {
    repository(fake, mirror, "north", at).into_iter().map(|(_, result)| result).collect()
}

#[test]
fn an_unchanged_repository_costs_only_conditional_reads_and_an_edited_one_or_a_moved_main_refetches_what_changed() {
    main_moving_compares_only_the_unlanded_branches_again_and_a_deleted_branch_loses_its_compare();
    let root = tempfile::tempdir().unwrap();
    let (fake, mirror) = (Fake::new(), Mirror::at(root.path().join("github-mirror.db")));
    assert_eq!(outcomes(&fake, &mirror, 100.0), vec![Ok(Fetch::Refreshed); 13]);
    assert_eq!(fake.graphql_reads.get(), 1);
    fake.sent_etags.borrow_mut().clear();
    assert_eq!(outcomes(&fake, &mirror, 200.0), vec![Ok(Fetch::Unchanged); 13]);
    assert_eq!(fake.graphql_reads.get(), 1);
    assert_eq!(*fake.sent_etags.borrow(), [Some("\"p1\"")].into_iter().chain([Some("\"r1\""); 11]).map(|etag| etag.map(str::to_owned)).collect::<Vec<_>>());
    assert_eq!(mirror.fresh(&key("north", OPEN_ISSUES), 200.0 + store::FRESH_S), Some(json!([{"number": 638, "title": "a local GitHub mirror", "edited": false}, {"number": 9}])));
    fake.issues_edited.set(true);
    assert_eq!(outcomes(&fake, &mirror, 300.0)[0], Ok(Fetch::Refreshed));
    assert_eq!(mirror.read(&key("north", OPEN_ISSUES)).unwrap().body[0]["edited"], true);
}

#[test]
fn gh_include_output_reads_as_unchanged_on_304_and_as_the_body_and_etag_on_200_and_a_pages_404_as_no_site() {
    a_404_from_the_pages_endpoint_means_no_site_and_any_other_failure_is_an_error();
    assert_eq!(response("HTTP/2.0 304 Not Modified\r\nEtag: W/\"abc\"\r\n\r\n"), Ok(Answer::Unchanged));
    let changed = "HTTP/2.0 200 OK\r\nContent-Type: application/json\r\nEtag: W/\"abc\"\r\nX-Ratelimit-Remaining: 4999\r\n\r\n[{\"id\":1}]";
    assert_eq!(response(changed), Ok(Answer::Changed { body: json!([{"id": 1}]), etag: Some("W/\"abc\"".into()) }));
    assert_eq!(response("HTTP/2.0 403 Forbidden\r\n\r\n{}"), Err("HTTP 403".into()));
}

#[test]
fn a_fresh_mirror_answers_branches_tags_releases_issue_states_pages_the_repository_list_and_compares_without_calling_github() {
    a_fresh_mirror_answers_pages_the_repository_list_and_branch_compares_without_calling_github();
    let root = tempfile::tempdir().unwrap();
    let (fake, mirror) = (Fake::new(), Mirror::at(root.path().join("github-mirror.db")));
    outcomes(&fake, &mirror, 100.0);
    let at = 100.0 + store::FRESH_S;
    let never = || -> Result<Vec<Value>, String> { panic!("a mirror hit must not call GitHub") };
    assert_eq!(read::listed(&mirror, at, "north", store::BRANCHES, never).unwrap(), vec![json!({"name": "main", "target": {"oid": "abc"}}), json!({"name": "topic", "target": {"oid": "t1"}}), json!({"name": "landed", "target": {"oid": "l1"}})]);
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


fn a_fresh_mirror_answers_pages_the_repository_list_and_branch_compares_without_calling_github() {
    let root = tempfile::tempdir().unwrap();
    let (fake, mirror) = (Fake::new(), Mirror::at(root.path().join("github-mirror.db")));
    outcomes(&fake, &mirror, 100.0);
    assert!(audit::account(&fake, &mirror, 100.0).1.is_ok());
    let at = 100.0 + store::FRESH_S;
    let never = || -> Result<Value, String> { panic!("a mirror hit must not call GitHub") };
    assert_eq!(read::pages_site(&mirror, at, "north", never).unwrap(), Value::Null, "a 404 from GitHub is stored as no Pages source");
    assert_eq!(read::repo_list(&mirror, at, never).unwrap(), json!([{"name": "north"}]));
    assert_eq!(read::compare(&mirror, "north", "topic", "abc", "t1", never).unwrap()["ahead_by"], 2);
    assert_eq!(read::compare(&mirror, "north", "topic", "abc", "t1", never).unwrap()["messages"], "Fix a thing (north#638)\n");
    assert_eq!(read::compare(&mirror, "north", "topic", "abc", "t2", || Ok(json!("live"))).unwrap(), json!("live"));
    assert_eq!(read::compare(&mirror, "north", "topic", "def", "t1", || Ok(json!("live"))).unwrap(), json!("live"), "an unlanded branch is compared again once main has moved");
    assert_eq!(read::compare(&mirror, "north", "landed", "def", "l1", never).unwrap()["ahead_by"], 0, "a landed branch stays landed whatever main becomes");
    assert_eq!(read::pages_site(&mirror, at + 1.0, "north", || Ok(json!("live"))).unwrap(), json!("live"));
}

fn main_moving_compares_only_the_unlanded_branches_again_and_a_deleted_branch_loses_its_compare() {
    let root = tempfile::tempdir().unwrap();
    let (fake, mirror) = (Fake::new(), Mirror::at(root.path().join("github-mirror.db")));
    outcomes(&fake, &mirror, 100.0);
    assert_eq!(*fake.compare_reads.borrow(), [["topic", "landed"].map(String::from).to_vec()]);
    outcomes(&fake, &mirror, 200.0);
    assert_eq!((fake.compare_reads.borrow().len(), fake.pages_reads.get()), (1, 1), "an unchanged repository costs no compare and no Pages read inside half an hour");
    fake.main_moved.set(true);
    mirror.write(&key("north", BRANCHES_PROBE), &json!([]), Some("stale"), 200.0).unwrap();
    outcomes(&fake, &mirror, 300.0);
    assert_eq!(fake.compare_reads.borrow()[1], vec!["topic".to_owned()]);
    outcomes(&fake, &mirror, 100.0 + REFETCH_S + 1.0);
    assert_eq!(fake.pages_reads.get(), 2, "the Pages source is read again after half an hour");
    mirror.write(&key("north", &audit::compare_kind("gone")), &json!({"head": "g1"}), None, 300.0).unwrap();
    mirror.write(&key("north", BRANCHES_PROBE), &json!([]), Some("stale"), 300.0).unwrap();
    outcomes(&fake, &mirror, 400.0 + REFETCH_S);
    assert!(mirror.read(&key("north", &audit::compare_kind("gone"))).is_none());
}

fn a_404_from_the_pages_endpoint_means_no_site_and_any_other_failure_is_an_error() {
    let site = json!({"source": {"branch": "gh-pages"}});
    assert_eq!(audit::pages_answer(Ok(Answer::Changed { body: site.clone(), etag: None }), "north"), Ok(site));
    assert_eq!(audit::pages_answer(Err("gh api repos/x/pages: HTTP 404 gh: Not Found (HTTP 404)".into()), "north"), Ok(Value::Null));
    assert!(audit::pages_answer(Err("HTTP 403".into()), "north").is_err());
}
