use super::{DEFAULTS, TREE, resolve, table};
use std::fs;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("north-harness-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn a_runtime_reads_its_tables_from_the_tree_pinned_beside_it_not_from_the_checkout() {
    let state = scratch("pinned");
    let runtime = state.join("runtimes").join("a".repeat(40));
    fs::create_dir_all(runtime.join(TREE).join("harness")).unwrap();
    fs::write(runtime.join(TREE).join(DEFAULTS), "[pinned]\nvalue = 1\n").unwrap();
    let checkout = state.join("home/code/north/main");
    fs::create_dir_all(checkout.join("harness")).unwrap();
    fs::write(checkout.join(DEFAULTS), "[checkout]\nvalue = 2\n").unwrap();
    assert_eq!(resolve(None, &runtime.join("north"), &state.join("home")), Ok(runtime.join(TREE)));
    assert_eq!(resolve(Some(checkout.clone()), &runtime.join("north"), &state.join("home")), Ok(checkout));
    fs::remove_dir_all(&state).unwrap();
}

#[test]
fn a_test_or_dev_build_reads_the_tables_of_the_source_tree_it_was_built_from() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = fs::read_to_string(root.join(DEFAULTS)).unwrap();
    assert_eq!(table(DEFAULTS), text);
    assert!(std::ptr::eq(table(DEFAULTS), table(DEFAULTS)));
}

#[test]
fn a_promoted_runtime_with_no_tree_never_falls_back_to_a_source_tree() {
    let state = scratch("treeless");
    let runtime = state.join("runtimes").join("b".repeat(40));
    fs::create_dir_all(&runtime).unwrap();
    let resolved = resolve(None, &runtime.join("north"), &state.join("home"));
    assert!(resolved.as_ref().is_err_and(|skew| skew.contains("has no tree")), "{resolved:?}");
    fs::remove_dir_all(&state).unwrap();
}
