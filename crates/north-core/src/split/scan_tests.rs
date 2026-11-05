use super::paths::glob;
use super::rewrite::replace_mention;
use super::scan::{code_only, declared_module, names_crate, paths_after, read};

#[test]
fn flattens_use_trees_after_a_prefix() {
    let text = "use crate::{fleet::{self, Fleet}, ledger as books};\nlet n = crate::ops::tempo::run();\nlet m = super::crate::nope;";
    assert_eq!(paths_after(text, "crate"), ["fleet::self", "fleet::Fleet", "ledger", "ops::tempo::run"]);
}

#[test]
fn names_a_crate_only_as_a_path_root_import_or_macro() {
    assert!(names_crate("let x = padding::pad(\"\");", "padding"));
    assert!(names_crate("use padding;", "padding"));
    assert!(names_crate("serde_json!(1)", "serde_json"));
    assert!(!names_crate("let padding = 1; my_padding::x();", "padding"));
    assert!(!names_crate("crate::padding::x()", "padding"));
}

#[test]
fn reads_test_files_inline_test_modules_and_gated_children_as_tests() {
    let files = [
        ("a/mod.rs".to_string(), "use x::prod;\n#[cfg(test)]\nmod helper;\n#[cfg(test)]\nmod tests {\n    use y::inline;\n}\n".to_string()),
        ("a/helper.rs".to_string(), "use z::gated;\n".to_string()),
        ("a/b_tests.rs".to_string(), "use w::named;\n/// ```\n/// doc\n/// ```\n".to_string()),
    ];
    let sources = read(&files);
    assert!(sources.prod.contains("x::prod") && !sources.prod.contains("inline") && !sources.prod.contains("gated"));
    assert!(sources.tests.contains("y::inline") && sources.tests.contains("z::gated") && sources.tests.contains("w::named"));
    assert!(sources.doctests);
}

#[test]
fn declares_modules_with_any_visibility() {
    assert_eq!(declared_module("pub(crate) mod fixtures;"), Some("fixtures"));
    assert_eq!(declared_module("    pub mod tui;"), Some("tui"));
    assert_eq!(declared_module("mod tests {"), None);
}

#[test]
fn globs_match_like_structure_toml() {
    assert!(glob("crates/*/src/lib.rs", "crates/north-tui/src/lib.rs"));
    assert!(glob("crates/north-core/src/ops/**", "crates/north-core/src/ops/tempo/mod.rs"));
    assert!(!glob("crates/north-core/src/ops/**", "crates/north-core/src/opsx/mod.rs"));
}

#[test]
fn rewrites_a_path_mention_only_at_a_path_boundary() {
    let text = "crates/north-core/src/tui/mod.rs crates/north-core/src/tui_old crates/north-core/src/tui`";
    assert_eq!(replace_mention(text, "crates/north-core/src/tui", "crates/north-tui/src/tui"), "crates/north-tui/src/tui/mod.rs crates/north-core/src/tui_old crates/north-tui/src/tui`");
}

#[test]
fn blanks_string_and_char_literals_but_keeps_code() {
    let text = "let a = \"crate::ops::tempo\"; let b = r#\"crate::x \"q\" \"#; let c = '\"'; crate::fleet::run(&'a x);";
    assert_eq!(paths_after(&code_only(text), "crate"), ["fleet::run"]);
    assert_eq!(code_only(text).len(), text.len());
}
