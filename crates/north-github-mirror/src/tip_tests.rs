use super::*;
use serde_json::json;

fn mirror_holding(root: &Path, at: f64) -> Mirror {
    let mirror = Mirror::at(root.join("github-mirror.db"));
    mirror.write(&key("north", BRANCHES), &json!([{"name": "main", "target": {"oid": "aaa"}}]), None, at).unwrap();
    mirror
}

#[test]
fn fresh_tip_equal_tips_skip_the_network() {
    let root = tempfile::tempdir().unwrap();
    let mirror = mirror_holding(root.path(), 1000.0);
    assert!(holds_every_tip(&mirror, "north", 1030.0, |_| Some("aaa".to_owned())));
}

#[test]
fn fresh_tip_differing_or_stale_tips_fetch() {
    let root = tempfile::tempdir().unwrap();
    let mirror = mirror_holding(root.path(), 1000.0);
    assert!(!holds_every_tip(&mirror, "north", 1030.0, |_| Some("bbb".to_owned())));
    assert!(!holds_every_tip(&mirror, "north", 1030.0, |_| None));
    assert!(!holds_every_tip(&mirror, "north", 1100.0, |_| Some("aaa".to_owned())));
}
