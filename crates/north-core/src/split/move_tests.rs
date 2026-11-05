use super::structure::{cycle, parse};
use super::workspace_tests::Fixture;
use super::{move_modules, report};

const TUI_DEPENDENCIES: &str = "[dependencies]\nnorth-base = { path = \"../north-base\" }\nnorth-core = { path = \"../north-core\" }\nleft-pad.workspace = true\n\n[dev-dependencies]\nunused-pad.workspace = true\n";

fn trailers(text: &str) -> Vec<String> {
    text.lines().filter(|line| line.starts_with("Dependency: ")).map(|line| line.split(" — ").next().unwrap().to_string()).collect()
}

#[test]
fn moves_tui_byte_identical_with_an_exact_manifest_and_a_judged_test_item() {
    let fixture = Fixture::new();
    let mut plan = fixture.plan("north-core", "north-tui", &["tui"], &["test_support"]);
    plan.dry_run = true;
    let dry = move_modules(&plan).unwrap();
    assert_eq!(fixture.status(), "", "a dry run writes nothing");
    plan.dry_run = false;
    let outcome = move_modules(&plan).unwrap();
    assert_eq!(outcome.manifest, dry.manifest);
    let renames = fixture.renames();
    assert_eq!(renames.len(), 3, "{renames:?}");
    assert!(renames.iter().all(|line| line.starts_with("R100\tcrates/north-core/src/tui/")), "{renames:?}");
    let manifest = fixture.read("crates/north-tui/Cargo.toml");
    assert!(manifest.contains(TUI_DEPENDENCIES), "{manifest}");
    assert!(manifest.contains("name = \"north-tui\"") && manifest.contains("[lib]\ndoctest = false\n"), "{manifest}");
    assert!(!manifest.contains("ghost-pad") && !manifest.contains("north_core::"), "{manifest}");
    assert_eq!(fixture.read("crates/north-tui/src/lib.rs"), "pub use north_core::{fleet, platform};\n\npub mod tui;\n");
    assert_eq!(trailers(&report(&outcome)), ["Dependency: north-tui", "Dependency: north-base", "Dependency: north-core", "Dependency: left-pad", "Dependency: unused-pad"]);
    let fleet = fixture.read("crates/north-core/src/fleet/mod.rs");
    assert!(fleet.contains("#[doc(hidden)]\npub mod test_support;") && !fleet.contains("cfg(test)"), "{fleet}");
    assert!(fleet.contains("pub fn count()") && fleet.contains("    pub name: String,"), "{fleet}");
    assert!(fixture.read("crates/north-core/src/fleet/test_support.rs").starts_with("pub fn sample()"));
    assert_eq!(outcome.test_items.len(), 1, "{:?}", outcome.test_items);
    assert!(outcome.test_items[0].starts_with("crates/north-core/src/fleet/mod.rs:"), "{:?}", outcome.test_items);
    assert!(outcome.widened.iter().any(|line| line.contains("pub fn count()")), "{:?}", outcome.widened);
}

#[test]
fn a_moved_tree_keeps_fixtures_and_structure_and_a_second_run_is_a_no_op() {
    let fixture = Fixture::new();
    move_modules(&fixture.plan("north-core", "north-tui", &["tui"], &["test_support"])).unwrap();
    let structure = parse(&fixture.read("structure.toml")).unwrap();
    assert_eq!(structure.may_use["crate-roots"], ["platform"]);
    assert_eq!(structure.paths["tui"], ["crates/north-tui/src/lib.rs", "crates/north-tui/src/tui/**"]);
    assert_eq!(cycle(&structure), None);
    assert!(fixture.read("crates/north-core/src/ledger/replay_tests.rs").contains("\"crates/north-core/src/tui/mod.rs\""));
    assert!(fixture.read("crates/north-core/src/ledger/fixtures/conflict.txt").contains("crates/north-core/src/tui/mod.rs"));
    assert_eq!(fixture.read("docs/notes.md"), "The screen lives in `crates/north-tui/src/tui/mod.rs` and reads crates/north-tui/src/tui/screen.txt.\n");
    let main = fixture.read("src/main.rs");
    assert!(main.starts_with("use north_core::{fleet, guard, ops};\nuse north_tui::tui;\n"), "{main}");
    let root = fixture.read("Cargo.toml");
    assert!(root.contains("north-core = { path = \"crates/north-core\" }\nnorth-tui = { path = \"crates/north-tui\" }\n"), "{root}");
    assert!(root.contains("members = [\"crates/north-base\", \"crates/north-core\", \"crates/north-tui\"]"), "{root}");
    fixture.commit("moved");
    let again = move_modules(&fixture.plan("north-core", "north-tui", &["tui"], &["test_support"])).unwrap();
    assert!(again.moved.is_empty());
    assert_eq!(fixture.status(), "");
    assert!(report(&again).starts_with("nothing to move"));
}

#[test]
fn refuses_to_widen_a_cfg_test_item_it_was_not_allowed() {
    let fixture = Fixture::new();
    let error = move_modules(&fixture.plan("north-core", "north-digest", &["digest"], &[])).unwrap_err();
    assert!(error.contains("#[cfg(test)] mod fleet::test_support") && error.contains("--allow-test-item fleet::test_support"), "{error}");
    assert!(fixture.read("crates/north-core/src/fleet/mod.rs").contains("#[cfg(test)]\npub(crate) mod test_support;"));
}

#[test]
fn moves_guard_and_hook_into_one_crate_in_one_run() {
    let fixture = Fixture::new();
    let outcome = move_modules(&fixture.plan("north-core", "north-guard", &["guard", "hook"], &[])).unwrap();
    let mut renames = fixture.renames();
    renames.sort();
    assert_eq!(renames, ["R100\tcrates/north-core/src/guard/mod.rs\tcrates/north-guard/src/guard/mod.rs", "R100\tcrates/north-core/src/hook.rs\tcrates/north-guard/src/hook.rs", "R100\tcrates/north-core/src/hook_tests.rs\tcrates/north-guard/src/hook_tests.rs"]);
    assert_eq!(fixture.read("crates/north-guard/src/lib.rs"), "pub use north_core::ledger;\n\npub mod guard;\npub mod hook;\n");
    assert!(fixture.read("crates/north-guard/Cargo.toml").contains("[dependencies]\nnorth-core = { path = \"../north-core\" }\n\n[lints]"));
    assert_eq!(trailers(&report(&outcome)), ["Dependency: north-guard", "Dependency: north-core"]);
    let structure = parse(&fixture.read("structure.toml")).unwrap();
    assert_eq!(structure.paths["guard"], ["crates/north-guard/src/lib.rs", "crates/north-guard/src/guard/**", "crates/north-guard/src/hook.rs", "crates/north-guard/src/hook_tests.rs"]);
    let core = fixture.read("crates/north-core/src/lib.rs");
    assert!(!core.contains("pub mod guard;") && !core.contains("pub mod hook;"), "{core}");
}

#[test]
fn moves_a_nested_module_out_of_a_crate_that_is_not_north_core() {
    let fixture = Fixture::new();
    move_modules(&fixture.plan("north-core", "north-ops", &["ops"], &[])).unwrap();
    fixture.commit("ops");
    let outcome = move_modules(&fixture.plan("north-ops", "north-tempo", &["ops/tempo"], &[])).unwrap();
    assert_eq!(outcome.moved, ["ops/tempo"]);
    assert_eq!(fixture.read("crates/north-tempo/src/lib.rs"), "pub use north_ops::ops;\n\npub mod tempo;\n");
    assert!(!fixture.read("crates/north-ops/src/ops/mod.rs").contains("tempo"));
    let main = fixture.read("src/main.rs");
    assert!(main.contains("crate::tempo::run()") && main.contains("use north_ops::ops;\nuse north_tempo::tempo;\n"), "{main}");
    let structure = parse(&fixture.read("structure.toml")).unwrap();
    assert_eq!(structure.paths["ops"], ["crates/north-tempo/src/lib.rs", "crates/north-ops/src/lib.rs", "crates/north-ops/src/ops/**", "crates/north-tempo/src/tempo/**"]);
    assert!(fixture.read("Cargo.toml").contains("north-ops = { path = \"crates/north-ops\" }\nnorth-tempo = { path = \"crates/north-tempo\" }\n"));
}
