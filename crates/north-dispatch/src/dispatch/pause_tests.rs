use north_base::dispatch_pause::Pause;
use north_base::foundation::clock::Timestamp;

#[test]
fn a_pause_lifts_itself_when_its_cause_check_passes_or_its_expiry_comes() {
    let at = |text: &str| Timestamp::parse_rfc3339(text).unwrap();
    let now = at("2026-11-11T12:00:00Z");
    let set = |until: Option<&str>, check: Option<&str>| Pause::set("account b is the last headroom", "lead", until, check, now);
    assert!(set(None, None).is_err(), "a pause with neither a cause check nor an expiry is refused");

    let by_check = set(None, Some("test -e /gone")).unwrap();
    assert_eq!(by_check.lifted_by(now.unix_seconds_f64(), |_| false), None, "a pause stands while its check fails");
    assert!(by_check.lifted_by(now.unix_seconds_f64(), |_| true).is_some_and(|why| why.contains("test -e /gone")));

    let by_expiry = set(Some("2026-11-11T13:00:00Z"), None).unwrap();
    assert_eq!(by_expiry.lifted_by(now.unix_seconds_f64(), |_| false), None, "a pause stands before its expiry");
    assert!(by_expiry.lifted_by(at("2026-11-11T13:00:01Z").unix_seconds_f64(), |_| false).is_some_and(|why| why.contains("expiry")));
}
