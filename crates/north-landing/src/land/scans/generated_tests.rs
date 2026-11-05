use super::{declared_output, removed_rows, rows, valid_path};

#[test]
fn rows_split_like_python_bytes_splitlines() {
    assert_eq!(rows(b"a\nb\r\nc\rd"), [&b"a"[..], b"b", b"c", b"d"]);
    assert_eq!(rows(b"a\n\n"), [&b"a"[..], b""]);
    assert!(rows(b"").is_empty());
}

#[test]
fn removed_rows_counts_the_multiset_difference() {
    assert_eq!(removed_rows(&[b"x", b"x", b"y"], &[b"x", b"z"]), 2);
}

#[test]
fn valid_path_refuses_globs_dots_and_absolute_paths() {
    assert!(valid_path("ts/a.tsv"));
    for path in ["", "/a", "a//b", "a/./b", "../a", "a/", "a*", "a?b", "[a]"] {
        assert!(!valid_path(path), "{path}");
    }
}

#[test]
fn bare_generated_input_key_declares_an_empty_output() {
    assert_eq!(declared_output("generated.input"), Some(""));
    assert_eq!(declared_output("generated.a.tsv.input"), Some("a.tsv"));
    assert_eq!(declared_output("generated.a.tsv.other"), None);
}
