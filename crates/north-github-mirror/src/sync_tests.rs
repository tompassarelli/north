use super::*;
use serde_json::json;
use std::cell::{Cell, RefCell};

struct Fake {
    issues_edited: Cell<bool>,
    graphql_fails: Cell<bool>,
    graphql_reads: Cell<u32>,
    sent_etags: RefCell<Vec<Option<String>>>,
}

impl Fake {
    fn new() -> Self {
        Self { issues_edited: Cell::new(false), graphql_fails: Cell::new(false), graphql_reads: Cell::new(0), sent_etags: RefCell::new(Vec::new()) }
    }
}

impl GitHub for Fake {
    fn get(&self, endpoint: &str, etag: Option<&str>) -> Result<Answer, String> {
        self.sent_etags.borrow_mut().push(etag.map(str::to_owned));
        let current = if endpoint.contains("/issues?") { if self.issues_edited.get() { "\"p2\"" } else { "\"p1\"" } } else { "\"r1\"" };
        if etag == Some(current) {
            return Ok(Answer::Unchanged);
        }
        Ok(Answer::Changed { body: json!({"endpoint": endpoint, "etag": current}), etag: Some(current.to_owned()) })
    }

    fn open_issues(&self, _repo: &str) -> Result<Vec<Value>, String> {
        self.graphql_reads.set(self.graphql_reads.get() + 1);
        if self.graphql_fails.get() {
            return Err("graphql down".into());
        }
        Ok(vec![json!({"number": 638, "title": "a local GitHub mirror", "edited": self.issues_edited.get()})])
    }
}

fn outcomes(fake: &Fake, mirror: &Mirror, at: f64) -> Vec<Result<Fetch, String>> {
    repository(fake, mirror, "north", at).into_iter().map(|(_, result)| result).collect()
}

#[test]
fn an_unchanged_repository_costs_only_conditional_reads_and_an_edited_one_refetches_its_open_issues() {
    let root = tempfile::tempdir().unwrap();
    let (fake, mirror) = (Fake::new(), Mirror::at(root.path().join("github-mirror.db")));
    assert_eq!(outcomes(&fake, &mirror, 100.0), vec![Ok(Fetch::Refreshed); 3]);
    assert_eq!(fake.graphql_reads.get(), 1);
    fake.sent_etags.borrow_mut().clear();
    assert_eq!(outcomes(&fake, &mirror, 200.0), vec![Ok(Fetch::Unchanged); 3]);
    assert_eq!(fake.graphql_reads.get(), 1);
    assert_eq!(*fake.sent_etags.borrow(), vec![Some("\"p1\"".to_owned()), Some("\"r1\"".to_owned()), Some("\"r1\"".to_owned())]);
    assert_eq!(mirror.fresh(&key("north", OPEN_ISSUES), 200.0 + store::FRESH_S), Some(json!([{"number": 638, "title": "a local GitHub mirror", "edited": false}])));
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
