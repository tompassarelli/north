use std::ffi::OsString;

use super::{carried, record};

fn environment(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs.iter().map(|(name, value)| (OsString::from(name), OsString::from(value))).collect()
}

/// north#1556: firn's authoring_native test set its module paths, FIRN_REPO and a fake-sops PATH for bun, and depbox's cleared environment dropped every one, so the land check failed "pure fixtures failed" while CI passed.
#[test]
fn a_bun_the_gate_sends_into_depbox_keeps_what_its_check_set_and_nothing_it_inherited() {
    let start = record(environment(&[("PATH", "/bin"), ("GH_TOKEN", "secret"), ("TMPDIR", "/tmp/north-check-a")]));
    let now = environment(&[("PATH", "/tmp/north-check-a/fake-bin:/bin"), ("GH_TOKEN", "secret"), ("TMPDIR", "/tmp/north-check-a"), ("FIRN_AUTHORING_MODULE", "/repo/native/authoring_native.ts"), ("DEPBOX_ADMITTED", "1")]);
    assert_eq!(carried(now, &start), ["FIRN_AUTHORING_MODULE", "PATH", "TMPDIR"]);
    let elsewhere = environment(&[("TMPDIR", "/tmp/n-other")]);
    assert!(carried(elsewhere, &start).is_empty(), "a TMPDIR set after the check started names no scratch the sandbox binds");
}
