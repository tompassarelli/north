use super::imports::imports_of;
use super::rust_modules::Crate;
use super::source::Source;
use super::{Existence, Request, SourceFile, answer};

fn tree(paths: &[&str]) -> Source {
    Source::tree(paths.iter().map(|path| (*path).to_owned()).collect())
}

fn app_crate() -> Vec<Crate> {
    Crate::from_manifests([("Cargo.toml", "[package]\nname = \"app\"\n")].into_iter())
}

fn rust_imports(path: &str, text: &str) -> Vec<String> {
    imports_of(path, text, &tree(LAYOUT), &app_crate())
}

const LAYOUT: &[&str] = &["Cargo.toml", "src/lib.rs", "src/link.rs", "src/util.rs"];

#[test]
fn use_super_inside_an_inline_mod_resolves_to_the_file_that_holds_it() {
    let text = "pub fn go() {}\n#[cfg(test)]\nmod tests {\n    use super::*;\n}\n";
    assert_eq!(rust_imports("src/link.rs", text), Vec::<String>::new());
}

#[test]
fn use_super_outside_an_inline_mod_reaches_the_parent_module() {
    assert_eq!(rust_imports("src/link.rs", "use super::util::helper;\n"), ["src/util.rs"]);
    let text = "mod tests {\n    use super::super::Shared;\n}\n";
    assert_eq!(rust_imports("src/link.rs", text), ["src/lib.rs"]);
}

#[test]
fn use_self_inside_an_inline_mod_resolves_to_the_file_that_holds_it() {
    let text = "mod inner {\n    use self::deep::Thing;\n}\n";
    assert_eq!(rust_imports("src/util.rs", text), Vec::<String>::new());
    let files = tree(&["src/lib.rs", "src/link.rs", "src/link/child.rs"]);
    let child = imports_of("src/link.rs", "use self::child::Item;\n", &files, &app_crate());
    assert_eq!(child, ["src/link/child.rs"]);
}

#[test]
fn comments_and_strings_do_not_open_or_close_inline_mods() {
    let text = "/* mod fake { */\n// mod other {\nconst S: &str = \"mod x {\";\nuse super::util::A;\n";
    assert_eq!(rust_imports("src/link.rs", text), ["src/util.rs"]);
}

#[test]
fn use_trees_expand_to_each_resolvable_path_once() {
    let text = "use crate::{util::{B, C as D}, link::E};\nuse std::fmt;\n";
    assert_eq!(rust_imports("src/lib.rs", text), ["src/util.rs", "src/link.rs"]);
}

#[test]
fn nested_crates_resolve_by_name_and_directory() {
    let manifests = [
        ("Cargo.toml", "[package]\nname = \"root-app\"\n"),
        ("tools/dep/Cargo.toml", "[package]\nname = \"dep-lib\"\n"),
    ];
    let crates = Crate::from_manifests(manifests.into_iter());
    let files = tree(&["tools/dep/src/lib.rs", "tools/dep/src/part.rs", "src/main.rs"]);
    let from_root = imports_of("src/main.rs", "use dep_lib::part::Thing;\n", &files, &crates);
    assert_eq!(from_root, ["tools/dep/src/part.rs"]);
    let inside = imports_of("tools/dep/src/part.rs", "use crate::Thing;\n", &files, &crates);
    assert_eq!(inside, ["tools/dep/src/lib.rs"]);
}

#[test]
fn typescript_specifiers_resolve_relative_files_and_index_modules() {
    let files = tree(&["src/a/main.ts", "src/a/dep.ts", "src/a/pkg/index.ts", "src/b/x.ts", "src/c.js"]);
    let text = "import { a } from './dep';\nimport b from \"./pkg\";\nimport '../b/x';\nconst c = require('../c.js');\nimport z from 'node:path';\n// import q from './dep2';\n";
    let found = imports_of("src/a/main.ts", text, &files, &[]);
    assert_eq!(found, ["src/a/dep.ts", "src/a/pkg/index.ts", "src/b/x.ts", "src/c.js"]);
}

#[test]
fn a_js_specifier_falls_back_to_its_typescript_source() {
    let files = tree(&["src/a.ts", "src/b.ts"]);
    assert_eq!(imports_of("src/a.ts", "export * from './b.js';", &files, &[]), ["src/b.ts"]);
}

#[test]
fn c_includes_resolve_beside_the_file_then_from_the_root() {
    let files = tree(&["src/a.c", "src/local.h", "include/shared.h"]);
    let text = "#include \"local.h\"\n#include \"include/shared.h\"\n#include <stdio.h>\n";
    assert_eq!(imports_of("src/a.c", text, &files, &[]), ["src/local.h", "include/shared.h"]);
}

#[test]
fn the_request_answers_in_file_order_for_a_tree_and_a_root() {
    let request: Request = serde_json::from_str(
        r#"{"exists":{"tree":["Cargo.toml","src/lib.rs","src/util.rs"]},
            "manifests":[{"path":"Cargo.toml","text":"[package]\nname = \"app\"\n"}],
            "files":[{"path":"src/lib.rs","text":"use crate::util::A;"},{"path":"src/util.rs","text":"use crate::A;"}]}"#,
    )
    .unwrap();
    assert_eq!(answer(request).imports, [vec!["src/util.rs".to_owned()], vec!["src/lib.rs".to_owned()]]);

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("b.ts"), "").unwrap();
    let request = Request {
        exists: Existence::Root(dir.path().to_string_lossy().into_owned()),
        manifests: Vec::new(),
        files: vec![SourceFile { path: "a.ts".into(), text: "import './b';".into() }],
    };
    assert_eq!(answer(request).imports, [vec!["b.ts".to_owned()]]);
}

#[test]
fn a_leading_byte_order_mark_does_not_hide_the_first_import() {
    let text = "\u{feff}use crate::util::A;\n";
    assert_eq!(rust_imports("src/lib.rs", text), ["src/util.rs"]);
}

#[test]
fn an_absolute_c_include_is_looked_up_inside_the_root_only() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let header = outside.path().join("zlib.h");
    std::fs::write(&header, "").unwrap();
    let source = Source::root(dir.path().to_path_buf());
    let text = format!("#include \"{}\"\n", header.display());
    assert_eq!(imports_of("a.c", &text, &source, &[]), Vec::<String>::new());
}
