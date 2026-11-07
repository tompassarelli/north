use super::imports::imports_of;
use super::rust_modules::Crate;
use super::source::Source;

#[test]
fn an_inline_call_into_platform_ssh_is_an_import_like_its_use_line() {
    let files = ["crates/north-base/src/lib.rs", "crates/north-base/src/platform/mod.rs", "crates/north-base/src/platform/ssh.rs", "crates/north-base/src/platform/process.rs", "crates/north-base/src/platform/ssh_master.rs"];
    let source = Source::tree(files.iter().map(|path| (*path).to_owned()).collect());
    let crates = Crate::from_manifests([("crates/north-base/Cargo.toml", "[package]\nname = \"north-base\"\n")].into_iter());
    let from = "crates/north-base/src/platform/ssh_master.rs";
    let text = "pub fn open() {\n    let _ = \"crate::platform::host::x()\";\n    super::process::run_within();\n}\nmod inner {\n    fn f() { north_base::platform::ssh::x(); }\n}\n";
    assert_eq!(imports_of(from, text, &source, &crates), ["crates/north-base/src/platform/process.rs", "crates/north-base/src/platform/ssh.rs"]);
}
