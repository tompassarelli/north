use super::*;

#[test]
fn a_test_stem_drops_one_script_suffix_and_then_test() {
    assert_eq!(test_stem("bin/tool"), "bin/tool");
    assert_eq!(test_stem("harness/lib/events.py"), "harness/lib/events");
    assert_eq!(test_stem("harness/lib/ledger.test.py"), "harness/lib/ledger");
    assert_eq!(test_stem("bin/x.test.mjs"), "bin/x");
}

#[test]
fn an_extensionless_path_matches_by_its_directory_and_name_and_a_dotted_one_by_its_name() {
    let tool = regex::Regex::new(&patterns("bin/tool")[0]).unwrap();
    assert!(tool.is_match("bash bin/tool\n") && !tool.is_match("bash bin/tool-server\n") && !tool.is_match("bash lib/tool\n"));
    let script = regex::Regex::new(&patterns("harness/scripts/task.sh")[0]).unwrap();
    assert!(script.is_match("bash ../task.sh") && !script.is_match("bash task.shx") && !script.is_match("bash taskXsh"));
}

#[test]
fn only_a_python_module_with_an_identifier_name_gets_an_import_pattern() {
    assert_eq!(patterns("harness/lib/events.py").len(), 2);
    assert_eq!(patterns("harness/lib/landing-health.py").len(), 1);
    assert_eq!(patterns("bin/tool").len(), 1);
    let import = regex::Regex::new(&format!("(?m){}", patterns("harness/lib/events.py")[1])).unwrap();
    assert!(import.is_match("import os, events as renamed\n") && import.is_match("    from events import value\n") && !import.is_match("import events_extra\n"));
}

#[test]
fn a_symlinked_path_directory_is_pinned_to_its_target() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("store");
    fs::create_dir(&target).unwrap();
    let link = dir.path().join("profile");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let missing = dir.path().join("missing");
    let path = std::env::join_paths([&link, &missing]).unwrap();
    let pinned: Vec<PathBuf> = std::env::split_paths(&pinned_path(&path)).collect();
    assert_eq!(pinned, vec![fs::canonicalize(&target).unwrap(), missing]);
}
