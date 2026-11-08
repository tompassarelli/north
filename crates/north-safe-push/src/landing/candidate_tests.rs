use super::*;

// KEEP(bug): the private-text judge's timeout refused north#868 twice on 2026-11-08; a scan that exits no verdict is a transient the next round retries; north#842
#[test]
fn a_scan_with_no_verdict_is_a_transient_and_a_refusing_scan_still_refuses() {
    assert!(matches!(classify("head", &Reached::ScansNoVerdict, ""), Verdict::Transient(_)));
    assert!(matches!(classify("head", &Reached::ScansRefused, ""), Verdict::Refused(_)));
}
