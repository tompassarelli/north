use north_base::foundation::main_run_verdict::{Kept, Verdict, judge};

const TIMEOUT_LOG: &str = "test\tTest\t2026-11-14T19:24:41.7710459Z [2026-11-14T19:24:41.767+00:00]         TIMEOUT [  60.001s] north::it golden_path::a_slow_case";
const FAILED_LOG: &str = "test\tTest\t2026-11-14T19:24:41.7710459Z [2026-11-14T19:24:41.767+00:00]         FAIL [   0.526s] north::it golden_path::a_failed_case";
const LEDGER_TIMEOUT: &str = "test\tTest\t2026-11-15T01:43:00.0750899Z [2026-11-15T01:43:00.064+00:00]         TIMEOUT [ 120.011s] north::it threads_ledger::the_ledger_host_over_ssh_spools_while_down_serves_a_stale_snapshot_and_flushes_in_order";
const LEDGER_FAILED: &str = "test\tTest\t2026-11-15T01:53:05.0078698Z [2026-11-15T01:53:03.177+00:00]         FAIL [ 104.790s] north::it threads_ledger::the_ledger_host_over_ssh_spools_while_down_serves_a_stale_snapshot_and_flushes_in_order";

fn log(text: &str) -> Kept {
    Kept::Log(text.to_owned())
}

#[test]
fn timeout_only_run_pages_nothing() {
    let none = |_| Ok(Vec::new());
    let timed_out = judge(true, || Ok(log(TIMEOUT_LOG)), none).unwrap();
    assert!(matches!(timed_out, Verdict::NotJudged(_)) && !timed_out.pages(), "the red-main page and the runtime trip both read this verdict: {timed_out:?}");
    let cut_off = judge(true, || Ok(Kept::CutOff("lost communication with the server".to_owned())), none).unwrap();
    assert!(!cut_off.pages(), "{cut_off:?}");
    assert!(judge(true, || Ok(log(FAILED_LOG)), none).unwrap().pages(), "a run that failed a case still pages");
    assert!(judge(true, || Err("gh: connection refused".to_owned()), none).is_err(), "a run that cannot be read is judged by nobody");
}

#[test]
fn a_case_that_times_out_on_three_runs_in_a_row_is_red_naming_it() {
    let case = vec![("north".to_owned(), "threads_ledger::the_ledger_host_over_ssh_spools_while_down_serves_a_stale_snapshot_and_flushes_in_order".to_owned())];
    let three = judge(true, || Ok(log(LEDGER_TIMEOUT)), |count| Ok(vec![log(LEDGER_TIMEOUT), log(LEDGER_FAILED)].into_iter().take(count).collect())).unwrap();
    assert_eq!(three, Verdict::Red(case), "on 2026-11-15 hosted runs on 4693f2ecc6, c38f47870a and da00e6f468 each timed out on this case and each was read as not judged");
    let two = judge(true, || Ok(log(LEDGER_TIMEOUT)), |_| Ok(vec![log(LEDGER_TIMEOUT)])).unwrap();
    assert!(matches!(two, Verdict::NotJudged(_)), "two runs in a row are still the runner's: {two:?}");
    let other = judge(true, || Ok(log(LEDGER_TIMEOUT)), |_| Ok(vec![log(LEDGER_TIMEOUT), log(TIMEOUT_LOG)])).unwrap();
    assert!(matches!(other, Verdict::NotJudged(_)), "a run between that timed out on another case breaks the repeat: {other:?}");
}
