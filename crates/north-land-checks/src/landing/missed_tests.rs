use super::refused;
use crate::platform::records_mirror::{ROOT, Records, STAMP, Stamp};
use std::fs;

fn mirrored(state: &std::path::Path, at: f64) {
    let root = state.join(ROOT);
    let stamp = Stamp { host: "whiterabbit".into(), state: "/home/tom/.local/state".into(), at };
    fs::create_dir_all(root.join(STAMP).parent().unwrap()).unwrap();
    fs::write(root.join(STAMP), serde_json::to_vec(&stamp).unwrap()).unwrap();
}

#[test]
fn a_stale_lander_mirror_is_refused_not_passed() {
    let state = tempfile::tempdir().unwrap();
    let now = 1_794_270_600.0;
    mirrored(state.path(), now - 26.0 * 3600.0);
    let records = Records::of(state.path(), "gex45");
    let why = refused(&records, "2026-11-09T00:30:00Z", now).expect("a 26 h old mirror cannot judge this window");
    assert!(why.contains("stale mirror"), "{why}");
    assert!(why.contains("whiterabbit"), "{why}");
    assert!(why.contains("run it there"), "{why}");

    let fresh = tempfile::tempdir().unwrap();
    mirrored(fresh.path(), now - 30.0);
    assert_eq!(refused(&Records::of(fresh.path(), "gex45"), "2026-11-09T00:30:00Z", now), None, "a mirror the fleet tick just refreshed still answers");

    let local = tempfile::tempdir().unwrap();
    mirrored(local.path(), now - 26.0 * 3600.0);
    assert_eq!(refused(&Records::of(local.path(), "whiterabbit"), "2026-11-09T00:30:00Z", now), None, "the lander host reads its own records, however old they are");
}
