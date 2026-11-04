use super::*;
use std::cell::RefCell;

const COMMIT_REFS: &str = "remote: fatal error in commit_refs\nTo github.com:o/r.git\n ! [remote rejected]     4f2c1e9a7b3d -> land-check/4f2c1e9a7b3d (failure)\nerror: failed to push some refs to 'github.com:o/r.git'\n";
const HTTP_502: &str = "error: RPC failed; HTTP 502 curl 22 The requested URL returned error: 502\nfatal: the remote end hung up unexpectedly\n";
const STALE_INFO: &str = " ! [rejected]        4f2c1e9a7b3d -> land-check/4f2c1e9a7b3d (stale info)\nerror: failed to push some refs\n";

fn run(results: Vec<(bool, &str)>) -> (RefPush, usize, bool) {
    let calls = RefCell::new(results.into_iter());
    let attempts = RefCell::new(0);
    let paused = RefCell::new(false);
    let outcome = push_land_ref(
        "+abc:refs/heads/land-check/abc",
        |_| {
            *attempts.borrow_mut() += 1;
            let (ok, stderr) = calls.borrow_mut().next().expect("no more recorded pushes");
            (ok, stderr.to_string())
        },
        || *paused.borrow_mut() = true,
    );
    (outcome, attempts.into_inner(), paused.into_inner())
}

#[test]
fn recorded_commit_refs_rejection_is_retried_once_and_lands() {
    assert_eq!(run(vec![(false, COMMIT_REFS), (true, "")]), (RefPush::Pushed, 2, true));
}

#[test]
fn recorded_server_error_is_retried_once() {
    assert_eq!(run(vec![(false, HTTP_502), (true, "")]), (RefPush::Pushed, 2, true));
}

#[test]
fn a_second_transient_rejection_is_named_infrastructure() {
    assert_eq!(run(vec![(false, COMMIT_REFS), (false, COMMIT_REFS)]), (RefPush::Infrastructure, 2, true));
}

#[test]
fn an_ordinary_rejection_is_not_retried() {
    assert_eq!(run(vec![(false, STALE_INFO)]), (RefPush::Failed, 1, false));
    assert!(!transient_rejection("error: HTTP 404 not found"));
}
