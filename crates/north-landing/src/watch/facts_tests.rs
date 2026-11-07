use super::*;
use crate::watch::{WatchContext, github};

const RUNS: &str = r#"{"workflow_runs":[{"id":9,"name":"ci","head_branch":"main","event":"push","status":"completed","conclusion":"success"}]}"#;

#[test]
fn a_304_from_the_runs_read_keeps_the_last_runs_and_raises_no_api_error() {
    let state = tempfile::tempdir().unwrap();
    let cache = state.path().join("north/watch/runs-wisp.json");
    assert_eq!(read_runs(Ok(RUNS.into()), &cache).unwrap().len(), 1);
    let unchanged = read_runs(Err("gh api: gh: HTTP 304".into()), &cache).unwrap();
    assert_eq!(unchanged.len(), 1);
    assert!(read_runs(Err("gh api: gh: HTTP 502".into()), &cache).is_err(), "other failures stay errors");
    let facts = RepoFacts { name: "wisp".into(), slug: "o/wisp".into(), runs: unchanged, ..RepoFacts::default() };
    let context = WatchContext { repos: vec![facts], ..WatchContext::default() };
    assert!(github::probe(&context).iter().all(|alert| !alert.key.ends_with(":api-error")));
}
