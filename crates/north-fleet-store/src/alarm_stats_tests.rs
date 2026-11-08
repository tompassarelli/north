use super::*;

#[test]
fn the_committed_labels_read_and_every_source_over_ten_percent_false_names_its_narrowing() {
    let stats = compute(&[], &labels(LABELS).unwrap(), 0);
    let text = render(&stats, "all");
    assert!(text.contains("north-health: narrowed: 6854b939d"), "{text}");
    assert!(!text.contains("not narrowed"), "{text}");
}
