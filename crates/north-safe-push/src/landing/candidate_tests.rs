use super::*;

#[test]
fn a_scan_with_no_verdict_is_a_transient_and_a_refusing_scan_still_refuses() {
    assert!(matches!(classify("head", &Reached::ScansNoVerdict, ""), Verdict::Transient(_)));
    assert!(matches!(classify("head", &Reached::ScansRefused, ""), Verdict::Refused(_)));
}
