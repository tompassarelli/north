use super::*;

#[test]
fn retain_deletes_days_past_retention_and_leaves_the_open_hour_for_appends() {
    let root = tempfile::tempdir().unwrap();
    let now = 1_794_719_000.0;
    let stamp = Timestamp::from_unix_seconds_f64(now).iso();
    let open = root.path().join("placement").join(&stamp[..10]).join(format!("{}.jsonl", &stamp[11..13]));
    let old = root.path().join("admission/2026-10-01/03.snapshots.jsonl");
    for file in [&open, &old] {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "{}\n").unwrap();
    }
    std::fs::write(root.path().join("placement/turn.lock"), "").unwrap();
    let retained = retain(root.path(), now, 14);
    assert_eq!(retained.to_string(), "1 days deleted");
    assert!(!old.parent().unwrap().exists());
    assert_eq!(std::fs::read_to_string(&open).unwrap(), "{}\n");
}
