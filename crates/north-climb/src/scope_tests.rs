use std::path::{Path, PathBuf};

use crate::scope::{self, COUNTING, DEFAULTS, EVALUATOR, WINNER};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn defaults() -> String {
    std::fs::read_to_string(root().join(DEFAULTS)).unwrap()
}

fn changed(before: &str, key: &str) -> String {
    let header = format!("[{key}]\n");
    assert!(before.contains(&header), "{DEFAULTS} has no table {key}");
    before.replacen(&header, &format!("{header}changed_by_a_candidate = 1\n"), 1)
}

fn keys(text: &str) -> Vec<String> {
    text.lines().filter_map(|line| line.strip_prefix('[')?.strip_suffix(']')).filter(|key| !key.contains('[')).map(str::to_owned).collect()
}

fn sources(path: &Path, found: &mut Vec<PathBuf>) {
    if path.is_dir() {
        for entry in std::fs::read_dir(path).unwrap() {
            sources(&entry.unwrap().path(), found);
        }
    } else if path.extension().is_some_and(|extension| extension == "rs") && !scope::is_test(&path.to_string_lossy()) {
        found.push(path.to_owned());
    }
}

#[test]
fn the_scope_guard_refuses_every_guarded_path_and_every_defaults_key_that_the_evaluator_the_settle_rule_or_the_ledger_counting_reads() {
    for prefix in EVALUATOR.iter().chain(WINNER).chain(COUNTING) {
        assert!(root().join(prefix).exists(), "the scope guard refuses {prefix}, which is not in the tree, so the code it meant is unguarded");
    }
    let before = defaults();
    let keys = keys(&before);
    let mut files = Vec::new();
    for prefix in EVALUATOR.iter().chain(WINNER).chain(COUNTING) {
        sources(&root().join(prefix), &mut files);
    }
    let mut read = 0;
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        for key in keys.iter().filter(|key| text.contains(&format!("\"{key}\""))) {
            read += 1;
            assert!(!scope::key_refusals(&before, &changed(&before, key)).is_empty(), "{} reads {DEFAULTS} key {key}, which a candidate may change: list the file in the key's consumers", file.display());
        }
    }
    assert!(read >= 7, "the inventory found {read} keys read by guarded code; replay alone reads 7");

    let bar = scope::key_refusals(&before, &changed(&before, "replay_outcome_shift_ratio"));
    assert_eq!(bar, [format!("{DEFAULTS} key replay_outcome_shift_ratio is read by crates/north-replay/src/bounds.rs (the evaluator), which a candidate may not change")]);
    let touches = [DEFAULTS.to_owned()];
    assert_eq!(scope::refusals(&touches, Some((&before, &changed(&before, "placement_drain_margin")))), Vec::<String>::new(), "a placement key in the same file stays open");
    assert_eq!(scope::refusals(&touches, None).len(), 1, "a defaults change with no contents is refused whole");

    let refused = scope::refusals(&["crates/north-pace/src/router/settle.rs".to_owned(), "crates/north-dispatch/src/tune/cells.rs".to_owned(), "crates/north-dispatch/src/dispatch/tier.rs".to_owned()], None);
    assert_eq!(refused.len(), 2, "{refused:?}");
    assert!(!scope::diff_refusals("--- a/tier.rs\n+++ b/tier.rs\n-        self.arms.settled(Tier::Sonnet, Tier::Opus)\n+        self.arms.cheaper(Tier::Sonnet, Tier::Opus)\n").is_empty());
}
