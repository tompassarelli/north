use super::matches;

#[test]
fn a_push_path_pattern_matches_like_a_bash_glob_where_a_star_crosses_slashes() {
    assert!(matches(b"src/**", b"src/deep/main.ts"));
    assert!(matches(b"src/*", b"src/deep/main.ts"));
    assert!(matches(b"*.md", b"docs/notes.md"));
    assert!(matches(b"Cargo.?ock", b"Cargo.lock"));
    assert!(!matches(b"src/**", b"content.txt"));
    assert!(!matches(b"Cargo.toml", b"Cargo.toml.bak"));
}

#[test]
fn a_push_path_pattern_honours_bracket_classes_and_escapes_like_bash() {
    assert!(matches(b"src/[ab]*.rs", b"src/a1.rs"));
    assert!(!matches(b"src/[!ab]*.rs", b"src/a1.rs"));
    assert!(matches(b"src/[0-9].rs", b"src/7.rs"));
    assert!(matches(br"docs/\*.md", b"docs/*.md"));
    assert!(!matches(br"docs/\*.md", b"docs/x.md"));
    assert!(matches(b"a[b", b"a[b"));
}

#[test]
fn many_stars_over_a_long_path_finish_without_backtracking_blowup() {
    let path = format!("{}c", "a/".repeat(4000));
    assert!(!matches(b"a/**/b/**/b/**/b/**/d", path.as_bytes()));
}
