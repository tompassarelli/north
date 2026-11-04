use super::Range;

fn sides(range: &str) -> Option<(&str, &str)> {
    Range::parse(range).map(|range| (range.base, range.head))
}

#[test]
fn range_splits_at_first_and_last_dots() {
    assert_eq!(sides("a..b"), Some(("a", "b")));
    assert_eq!(sides("a...b"), Some(("a", "b")));
    assert_eq!(sides("a..x..b"), Some(("a", "b")));
    assert_eq!(sides("a.."), Some(("a", "")));
}

#[test]
fn range_without_base_is_no_range() {
    assert_eq!(sides("..b"), None);
    assert_eq!(sides("ab"), None);
    assert_eq!(sides(""), None);
}
