use super::*;
use crate::hunks::parse;

fn test(name: &str) -> Test {
    Test { binary: "north-base".to_owned(), name: name.to_owned() }
}

fn map() -> Map {
    let function = |file: &str, start, end, reached_by: &[u32]| Function { file: file.to_owned(), start, end, reached_by: reached_by.to_vec() };
    Map {
        commit: "0123456789abcdef".to_owned(),
        recorded: 0,
        tests: vec![test("a"), test("b"), test("c"), test("d")],
        functions: vec![function("src/x.rs", 1, 10, &[0, 1]), function("src/x.rs", 4, 6, &[1]), function("src/x.rs", 20, 30, &[2]), function("src/y.rs", 1, 5, &[3])],
    }
}

fn diff(file: &str, hunks: &[&str]) -> Hunks {
    parse(&format!("--- a/{file}\n+++ b/{file}\n{}\n", hunks.join("\n")))
}

fn picked(map: &Map, since_map: Option<&Hunks>, lane: &Hunks) -> Selection {
    select(&Change { map, since_map, lane, other_paths: &[] })
}

fn tests(names: &[&str]) -> BTreeSet<Test> {
    names.iter().map(|name| test(name)).collect()
}

#[test]
fn a_change_inside_one_function_picks_only_the_tests_reaching_its_innermost_function() {
    let map = map();
    let since = Hunks::new();
    assert_eq!(picked(&map, Some(&since), &diff("src/x.rs", &["@@ -5 +5 @@"])), Selection::Tests { tests: tests(&["b"]), file_level: vec![] });
    assert_eq!(picked(&map, Some(&since), &diff("src/x.rs", &["@@ -25,0 +26,3 @@"])), Selection::Tests { tests: tests(&["c"]), file_level: vec![] });
}

#[test]
fn a_change_in_a_function_changed_since_the_map_falls_back_to_the_files_tests() {
    let map = map();
    let file_level = Selection::Tests { tests: tests(&["a", "b", "c"]), file_level: vec!["src/x.rs".to_owned()] };
    assert_eq!(picked(&map, Some(&diff("src/x.rs", &["@@ -22 +22 @@"])), &diff("src/x.rs", &["@@ -22 +22 @@"])), file_level);
    assert_eq!(picked(&map, Some(&diff("src/x.rs", &["@@ -22 +22,3 @@"])), &diff("src/x.rs", &["@@ -30 +30 @@"])), file_level);
}

#[test]
fn an_untraced_line_an_unknown_map_commit_or_a_file_or_input_the_map_never_saw_is_unmapped() {
    let map = map();
    assert!(matches!(picked(&map, Some(&Hunks::new()), &diff("src/x.rs", &["@@ -15 +15 @@"])), Selection::Unmapped(_)));
    assert!(matches!(picked(&map, None, &diff("src/x.rs", &["@@ -5 +5 @@"])), Selection::Unmapped(_)));
    assert!(matches!(picked(&map, Some(&Hunks::new()), &diff("src/new.rs", &["@@ -0,0 +1,3 @@"])), Selection::Unmapped(_)));
    assert!(matches!(picked(&map, Some(&Hunks::new()), &diff("Cargo.toml", &["@@ -3 +3 @@"])), Selection::Unmapped(_)));
    let other = ["harness/defaults.toml".to_owned()];
    assert!(matches!(select(&Change { map: &map, since_map: Some(&Hunks::new()), lane: &Hunks::new(), other_paths: &other }), Selection::Unmapped(_)));
}

#[test]
fn a_lane_touching_only_north_climb_still_selects_the_whole_tree_habits_test() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let queue = std::process::Command::new("git").arg("-C").arg(&root).args(["config", "--file", ".safe-push", "--get-all", "land.queue"]).output().unwrap();
    let always = crate::always::named(&String::from_utf8_lossy(&queue.stdout).lines().map(str::to_owned).collect::<Vec<_>>());
    let habits = Test { binary: "north-guard".to_owned(), name: "code_health::habits_tests::the_north_tree_keeps_its_rust_habits".to_owned() };
    let climb = Test { binary: "north-climb".to_owned(), name: "scope::tests::a_scope_reads".to_owned() };
    let map = Map { commit: "0123456789abcdef".to_owned(), recorded: 0, tests: vec![climb.clone()], functions: vec![Function { file: "crates/north-climb/src/scope.rs".to_owned(), start: 1, end: 10, reached_by: vec![0] }] };
    let selection = picked(&map, Some(&Hunks::new()), &diff("crates/north-climb/src/scope.rs", &["@@ -5 +5 @@"]));
    assert_eq!(selection, Selection::Tests { tests: BTreeSet::from([climb.clone()]), file_level: vec![] });
    assert_eq!(before_push(&selection, &always, 400), always.iter().cloned().chain([climb]).collect::<BTreeSet<_>>());
    assert!(always.contains(&habits), "{always:?}");
    let unmapped = picked(&map, Some(&Hunks::new()), &diff("crates/north-climb/src/scope.rs", &["@@ -15 +15 @@"]));
    assert!(matches!(unmapped, Selection::Unmapped(_)));
    assert_eq!(before_push(&unmapped, &always, 400), always);
}
