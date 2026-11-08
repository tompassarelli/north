use super::{Pass, Store, sync};
use crate::foundation::clock::Timestamp;

const WEAK: &str = "aa11";
const STRONG: &str = "bb22";
const SHA: &str = "cc33";

fn stores() -> (tempfile::TempDir, Store, Store) {
    let scratch = tempfile::tempdir().unwrap();
    let (first, second) = (Store(scratch.path().join("first")), Store(scratch.path().join("second")));
    (scratch, first, second)
}

fn passed(at: i64) -> Pass {
    Pass { at: Timestamp::from_unix_seconds(at), sha: SHA.to_owned() }
}

#[test]
fn a_pass_and_its_path_set_recorded_on_one_store_are_found_on_the_other_after_sync_and_a_fail_clears_both() {
    let (_scratch, first, second) = stores();
    let id = first.record_set(WEAK, b"path set bytes\0not parsed").unwrap();
    first.record_pass(WEAK, STRONG, &passed(100), "cpx62").unwrap();
    assert_eq!(second.pass(WEAK, STRONG).unwrap(), None);

    let report = sync(&first, &second).unwrap();
    assert_eq!((report.sent, report.received, report.cleared), (2, 0, 0));
    assert_eq!(second.pass(WEAK, STRONG).unwrap(), Some((passed(100), "cpx62".to_owned())));
    assert_eq!(second.sets(WEAK).unwrap(), vec![b"path set bytes\0not parsed".to_vec()]);
    assert_eq!(id.len(), 64);

    second.clear(WEAK, STRONG).unwrap();
    assert_eq!(second.pass(WEAK, STRONG).unwrap(), None);
    assert_eq!(first.pass(WEAK, STRONG).unwrap().map(|(pass, _)| pass), Some(passed(100)));
    let report = sync(&first, &second).unwrap();
    assert_eq!((report.sent, report.received, report.cleared), (0, 1, 1));
    assert_eq!(first.pass(WEAK, STRONG).unwrap(), None);
    assert_eq!(second.pass(WEAK, STRONG).unwrap(), None);
    assert_eq!(sync(&first, &second).unwrap(), Default::default());
}

#[test]
fn a_recorded_pass_is_never_rewritten_and_a_cleared_one_is_never_recorded_again() {
    let (_scratch, first, _second) = stores();
    first.record_pass(WEAK, STRONG, &passed(100), "cpx62").unwrap();
    first.record_pass(WEAK, STRONG, &passed(200), "gex45").unwrap();
    assert_eq!(first.pass(WEAK, STRONG).unwrap(), Some((passed(100), "cpx62".to_owned())));
    first.clear(WEAK, STRONG).unwrap();
    first.record_pass(WEAK, STRONG, &passed(300), "cpx62").unwrap();
    assert_eq!(first.pass(WEAK, STRONG).unwrap(), None);
}

#[test]
fn keys_that_are_not_lowercase_hex_are_refused() {
    let (_scratch, first, _second) = stores();
    assert!(first.record_set("../x", b"").is_err());
    assert!(first.record_set("AA11", b"").is_err());
    assert!(first.record_pass(WEAK, "b/../b", &passed(1), "h").is_err());
    assert!(first.pass(WEAK, "ZZ").is_err());
    assert!(first.clear("", STRONG).is_err());
}
