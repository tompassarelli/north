use std::path::Path;

use super::*;

/// On 2026-11-09 two workers moved to another account by hand kept their `usage:` wait for the account they left, so each
/// was listed parked and would have been sent that account's resume message at its reset (north#1007).
#[test]
fn a_session_that_left_an_account_loses_that_accounts_usage_wait_and_keeps_every_other_wait() {
    let home = tempfile::tempdir().unwrap();
    let waits = Waits::in_root(home.path());
    let usage = |account: &str| Run::Usage { account: account.into(), at_ms: 1_794_000_000_000 };
    let job = Run::Job { name: "build".into(), host: None };
    for run in [usage("e"), usage("b"), job.clone()] {
        waits.register("worker", run, Path::new("/tmp"), None).unwrap();
    }

    waits.clear_usage_off("worker", "b").unwrap();

    let kept: Vec<Run> = waits.of("worker").into_iter().map(|wait| wait.run).collect();
    assert_eq!(kept, [usage("b"), job]);
}
