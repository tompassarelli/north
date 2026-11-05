use std::fs;
use std::path::Path;

use super::{Plan, git};

const MANIFEST: &str = r#"[workspace]
members = ["crates/north-base", "crates/north-core"]
default-members = [".", "crates/north-base", "crates/north-core"]

[workspace.dependencies]
left-pad = { path = "vendor/left-pad" }
unused-pad = { path = "vendor/unused-pad" }
ghost-pad = { path = "vendor/ghost-pad" }

[workspace.lints.rust]
unsafe_code = "forbid"

[package]
name = "north"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
north-core = { path = "crates/north-core" }

[lints]
workspace = true
"#;

const CORE_MANIFEST: &str = r#"[package]
name = "north-core"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
north-base = { path = "../north-base" }
left-pad.workspace = true
unused-pad.workspace = true
ghost-pad.workspace = true

[lints]
workspace = true
"#;

const STRUCTURE: &str = r#"[thresholds]
file_lines = 500

[[module]]
name = "platform"
paths = ["crates/north-base/src/platform.rs"]
may_use = []

[[module]]
name = "fleet"
paths = ["crates/north-core/src/fleet/**"]
may_use = ["platform", "crate-roots"]

[[module]]
name = "ledger"
paths = ["crates/north-core/src/ledger/**", "crates/north-core/src/digest/**"]
may_use = ["fleet", "crate-roots"]

[[module]]
name = "guard"
paths = ["crates/north-core/src/guard/**", "crates/north-core/src/hook.rs", "crates/north-core/src/hook_tests.rs"]
may_use = ["ledger", "crate-roots"]

[[module]]
name = "ops"
paths = ["crates/north-core/src/ops/**"]
may_use = ["crate-roots"]

[[module]]
name = "tui"
paths = ["crates/north-core/src/tui/**"]
may_use = ["fleet", "platform", "crate-roots"]

[[module]]
name = "cli"
paths = ["src/**"]
may_use = ["tui", "fleet", "guard", "ops", "crate-roots"]

[[module]]
name = "crate-roots"
paths = ["crates/*/src/lib.rs"]
may_use = ["platform"]
"#;

const FILES: &[(&str, &str)] = &[
    ("Cargo.toml", MANIFEST),
    ("structure.toml", STRUCTURE),
    ("docs/notes.md", "The screen lives in `crates/north-core/src/tui/mod.rs` and reads crates/north-core/src/tui/screen.txt.\n"),
    ("src/main.rs", "use north_core::{fleet, guard, ops, tui};\n\nfn main() {\n    println!(\"{}\", tui::render(&fleet::Fleet::new(\"main\")));\n    println!(\"{}\", guard::allowed(\"ls\"));\n    println!(\"{}\", crate::ops::tempo::run() + ops::flow());\n}\n"),
    ("vendor/left-pad/Cargo.toml", "[package]\nname = \"left-pad\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\nname = \"padding\"\n"),
    ("vendor/left-pad/src/lib.rs", "pub fn pad(text: &str) -> String {\n    format!(\" {text}\")\n}\n"),
    ("vendor/unused-pad/Cargo.toml", "[package]\nname = \"unused-pad\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
    ("vendor/unused-pad/src/lib.rs", "pub fn width() -> usize {\n    1\n}\n"),
    ("vendor/ghost-pad/Cargo.toml", "[package]\nname = \"ghost-pad\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
    ("vendor/ghost-pad/src/lib.rs", "pub fn ghost() {}\n"),
    ("crates/north-base/Cargo.toml", "[package]\nname = \"north-base\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[lints]\nworkspace = true\n"),
    ("crates/north-base/src/lib.rs", "pub mod platform;\n"),
    ("crates/north-base/src/platform.rs", "pub fn home() -> String {\n    \"home\".into()\n}\n"),
    ("crates/north-core/Cargo.toml", CORE_MANIFEST),
    ("crates/north-core/src/lib.rs", "pub use north_base::platform;\n\npub mod digest;\npub mod fleet;\npub mod guard;\npub mod hook;\npub mod ledger;\npub mod ops;\npub mod tui;\n\npub fn ghost() {\n    ghost_pad::ghost();\n    let _ = padding::pad(\"\");\n    let _ = unused_pad::width();\n}\n"),
    ("crates/north-core/src/fleet/mod.rs", "pub struct Fleet {\n    pub(crate) name: String,\n}\n\nimpl Fleet {\n    pub fn new(name: &str) -> Fleet {\n        Fleet { name: name.to_string() }\n    }\n}\n\npub(crate) fn count() -> usize {\n    3\n}\n\n#[cfg(test)]\npub(crate) mod test_support;\n"),
    ("crates/north-core/src/fleet/test_support.rs", "pub(crate) fn sample() -> super::Fleet {\n    super::Fleet::new(\"sample\")\n}\n"),
    ("crates/north-core/src/tui/mod.rs", "use crate::fleet::Fleet;\n\npub const SCREEN: &str = include_str!(\"screen.txt\");\n\npub fn render(fleet: &Fleet) -> String {\n    format!(\"{}{}{}{}\", SCREEN.trim(), padding::pad(&fleet.name), crate::fleet::count(), north_base::platform::home())\n}\n\n#[cfg(test)]\nmod tests;\n"),
    ("crates/north-core/src/tui/screen.txt", "screen\n"),
    ("crates/north-core/src/tui/tests.rs", "#[test]\nfn renders_the_sample() {\n    let text = super::render(&crate::fleet::test_support::sample());\n    assert!(text.contains(\"sample\"));\n    assert_eq!(unused_pad::width(), 1);\n    assert_eq!(crate::platform::home(), \"home\");\n}\n"),
    ("crates/north-core/src/digest/mod.rs", "pub fn line() -> String {\n    \"digest\".into()\n}\n\n#[cfg(test)]\nmod tests;\n"),
    ("crates/north-core/src/digest/tests.rs", "#[test]\nfn digests_the_sample() {\n    assert_eq!(super::line(), \"digest\");\n    let _ = crate::fleet::test_support::sample();\n}\n"),
    ("crates/north-core/src/ledger/mod.rs", "pub fn size() -> usize {\n    1\n}\n\n#[cfg(test)]\nmod replay_tests;\n"),
    ("crates/north-core/src/ledger/replay_tests.rs", "const OLD: &str = \"crates/north-core/src/tui/mod.rs\";\n\n#[test]\nfn replays_the_old_path() {\n    assert!(include_str!(\"fixtures/conflict.txt\").contains(OLD));\n}\n"),
    ("crates/north-core/src/ledger/fixtures/conflict.txt", "<<<<<<< crates/north-core/src/tui/mod.rs\n"),
    ("crates/north-core/src/guard/mod.rs", "pub fn allowed(command: &str) -> bool {\n    crate::hook::event(command).len() > crate::ledger::size()\n}\n"),
    ("crates/north-core/src/hook.rs", "pub fn event(command: &str) -> String {\n    format!(\"hook:{command}\")\n}\n\npub fn guarded(command: &str) -> bool {\n    crate::guard::allowed(command)\n}\n\n#[cfg(test)]\n#[path = \"hook_tests.rs\"]\nmod tests;\n"),
    ("crates/north-core/src/hook_tests.rs", "#[test]\nfn guards_through_the_hook() {\n    assert!(super::guarded(\"ls\"));\n}\n"),
    ("crates/north-core/src/ops/mod.rs", "pub mod tempo;\n\npub fn flow() -> u32 {\n    7\n}\n"),
    ("crates/north-core/src/ops/tempo/mod.rs", "pub fn run() -> u32 {\n    crate::ops::flow() + 1\n}\n\n#[cfg(test)]\nmod tests;\n"),
    ("crates/north-core/src/ops/tempo/tests.rs", "#[test]\nfn runs() {\n    assert_eq!(super::run(), 8);\n}\n"),
];

pub(super) struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    pub(super) fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in FILES {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let fixture = Fixture { dir };
        let lock = crate::platform::process::run("cargo", Some(fixture.root()), &["generate-lockfile", "--offline"]).unwrap();
        assert!(lock.status.success(), "{}", String::from_utf8_lossy(&lock.stderr));
        fixture.git(&["init", "-q", "-b", "main"]);
        fixture.commit("fixture");
        fixture
    }

    pub(super) fn root(&self) -> &Path {
        self.dir.path()
    }

    pub(super) fn plan(&self, from: &str, to: &str, modules: &[&str], allow: &[&str]) -> Plan {
        Plan {
            root: self.root().to_path_buf(),
            from: from.into(),
            to: to.into(),
            modules: modules.iter().map(|module| module.to_string()).collect(),
            dry_run: false,
            allow_test_items: allow.iter().map(|item| item.to_string()).collect(),
            target_dir: Some(self.root().join("target")),
        }
    }

    pub(super) fn read(&self, path: &str) -> String {
        fs::read_to_string(self.root().join(path)).unwrap()
    }

    pub(super) fn git(&self, arguments: &[&str]) -> String {
        let mut all = vec!["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false"];
        all.extend_from_slice(arguments);
        git(self.root(), &all).unwrap()
    }

    pub(super) fn commit(&self, message: &str) {
        fs::write(self.root().join(".gitignore"), "target/\n").unwrap();
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", message]);
    }

    pub(super) fn renames(&self) -> Vec<String> {
        self.git(&["add", "-A"]);
        self.git(&["diff", "--cached", "-M", "--name-status", "HEAD"]).lines().filter(|line| line.starts_with('R')).map(str::to_string).collect()
    }

    pub(super) fn status(&self) -> String {
        self.git(&["status", "--porcelain"])
    }
}
