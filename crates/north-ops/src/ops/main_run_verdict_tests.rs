use north_base::foundation::main_run_verdict::{Kept, Verdict, judge};

const TIMEOUT_LOG: &str = "test\tTest\t2026-11-14T19:24:41.7710459Z [2026-11-14T19:24:41.767+00:00]         TIMEOUT [  60.001s] north::it golden_path::a_slow_case";
const FAILED_LOG: &str = "test\tTest\t2026-11-14T19:24:41.7710459Z [2026-11-14T19:24:41.767+00:00]         FAIL [   0.526s] north::it golden_path::a_failed_case";

#[test]
fn timeout_only_run_pages_nothing() {
    let timed_out = judge(true, || Ok(Kept::Log(TIMEOUT_LOG.to_owned()))).unwrap();
    assert!(matches!(timed_out, Verdict::NotJudged(_)) && !timed_out.pages(), "the red-main page and the runtime trip both read this verdict: {timed_out:?}");
    let cut_off = judge(true, || Ok(Kept::CutOff("lost communication with the server".to_owned()))).unwrap();
    assert!(!cut_off.pages(), "{cut_off:?}");
    assert!(judge(true, || Ok(Kept::Log(FAILED_LOG.to_owned()))).unwrap().pages(), "a run that failed a case still pages");
    assert!(judge(true, || Err("gh: connection refused".to_owned())).is_err(), "a run that cannot be read is judged by nobody");
}
