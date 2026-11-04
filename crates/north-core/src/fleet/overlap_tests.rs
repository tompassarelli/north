use std::collections::BTreeSet;

use super::*;

fn lane(name: &str, task: Option<&str>, since_ms: u64, files: &[&str]) -> Lane {
    Lane { name: name.into(), task: task.map(str::to_owned), since_ms, files: files.iter().map(|file| (*file).to_owned()).collect::<BTreeSet<_>>() }
}

fn none(_: &str) -> bool {
    false
}

fn declared(files: &[&str]) -> Vec<String> {
    files.iter().map(|file| (*file).to_owned()).collect()
}

#[test]
fn the_files_line_yields_paths_and_directories_without_the_notes_beside_them() {
    let brief = "Item: north#1 x\nFiles: src/cli/fleet/ (launch), crates/north-core/src/ops/dispatch/, threads queue follow, .gitattributes (regenerate driver, docs-6 in flight)\nPayback: y";
    assert_eq!(declared_files(brief), [".gitattributes", "crates/north-core/src/ops/dispatch/", "src/cli/fleet/"]);
    assert_eq!(declared_files("Files: `a/b.rs`, `a/b.rs`, north docs/rationale/, e.g. nothing."), ["a/b.rs", "docs/rationale/"]);
    assert!(declared_files("Item: x\nno files line").is_empty());
}

#[test]
fn a_named_file_and_a_named_directory_both_match_the_changed_files_under_them() {
    let lanes = [lane("a", Some("north#1"), 1, &["src/cli/fleet/router.rs", "src/cli/flow.rs", "docs/features.md"])];
    let found = overlaps(&declared(&["src/cli/fleet/", "docs/features.md"]), &lanes, &none);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].files, ["docs/features.md", "src/cli/fleet/router.rs"]);
    assert!(overlaps(&declared(&["src/cli/fleet"]), &lanes, &none).iter().all(|overlap| overlap.files == ["src/cli/fleet/router.rs"]));
    assert!(overlaps(&declared(&["src/cli/flee"]), &lanes, &none).is_empty());
}

#[test]
fn no_overlap_is_clear_and_one_lane_on_a_file_warns_without_queueing() {
    let lanes = [lane("a", Some("north#1"), 1, &["x.rs"])];
    assert_eq!(assess(&declared(&["y.rs"]), &lanes, &none, Policy::default()), Verdict::Clear);
    let verdict = assess(&declared(&["x.rs"]), &lanes, &none, Policy::default());
    assert!(matches!(&verdict, Verdict::Warn(found) if found.len() == 1));
    assert_eq!(warnings(&verdict), ["overlap: lane a also changes x.rs"]);
}

#[test]
fn two_lanes_on_one_file_queue_the_box_behind_the_earliest_lane_that_has_a_task() {
    let lanes = [lane("late", Some("north#3"), 30, &["x.rs"]), lane("early", Some("north#2"), 10, &["x.rs", "z.rs"]), lane("middle", None, 20, &["x.rs"])];
    let Verdict::Queue { behind, hot, overlaps } = assess(&declared(&["x.rs"]), &lanes, &none, Policy::default()) else { panic!("two lanes on x.rs queue the box") };
    assert_eq!((behind.lane.as_str(), behind.task.as_deref()), ("early", Some("north#2")));
    assert_eq!(hot, ["x.rs"]);
    assert_eq!(overlaps.iter().map(|overlap| overlap.lane.as_str()).collect::<Vec<_>>(), ["early", "middle", "late"]);
    assert_eq!(follows_line(&behind, &hot).as_deref(), Some("Follows: north#2 (lane early changes x.rs)"));
}

#[test]
fn the_earliest_overlapping_lane_without_a_task_is_skipped_for_the_earliest_one_that_has_one() {
    let lanes = [lane("first", None, 1, &["x.rs"]), lane("second", Some("north#9"), 2, &["x.rs"])];
    assert!(matches!(assess(&declared(&["x.rs"]), &lanes, &none, Policy::default()), Verdict::Queue { behind, .. } if behind.lane == "second"));
    let untasked = [lane("first", None, 1, &["x.rs"]), lane("second", None, 2, &["x.rs"])];
    assert!(matches!(assess(&declared(&["x.rs"]), &untasked, &none, Policy::default()), Verdict::Warn(found) if found.len() == 2));
}

#[test]
fn a_file_changed_by_two_lanes_is_hot_only_when_they_changed_that_file() {
    let lanes = [lane("a", Some("north#1"), 1, &["x.rs"]), lane("b", Some("north#2"), 2, &["y.rs"])];
    assert!(matches!(assess(&declared(&["x.rs", "y.rs"]), &lanes, &none, Policy::default()), Verdict::Warn(found) if found.len() == 2));
}

#[test]
fn the_setting_turns_queueing_off_and_moves_the_threshold() {
    let lanes = [lane("a", Some("north#1"), 1, &["x.rs"]), lane("b", Some("north#2"), 2, &["x.rs"])];
    let off = Policy { follows_on_overlap: false, ..Policy::default() };
    assert!(matches!(assess(&declared(&["x.rs"]), &lanes, &none, off), Verdict::Warn(_)));
    let eager = Policy { hot_lanes: 1, ..Policy::default() };
    assert!(matches!(assess(&declared(&["x.rs"]), &lanes[..1], &none, eager), Verdict::Queue { .. }));
    let wary = Policy { hot_lanes: 3, ..Policy::default() };
    assert!(matches!(assess(&declared(&["x.rs"]), &lanes, &none, wary), Verdict::Warn(_)));
}

#[test]
fn files_a_merge_driver_regenerates_are_not_overlaps() {
    let driven = MergeDriven::parse("# comment\nstructure.toml merge=set\ndocs/features.md merge=set\n*.lock merge=ours\nvendor/** merge=set\nnotes.txt merge=text\ngen.mjs linguist-generated=true\n");
    for path in ["structure.toml", "docs/features.md", "crates/x/Cargo.lock", "vendor/a/b.rs"] {
        assert!(driven.covers(path), "{path}");
    }
    for path in ["docs/features.md.bak", "src/structure.toml.rs", "notes.txt", "gen.mjs", "docs/other.md"] {
        assert!(!driven.covers(path), "{path}");
    }
    let lanes = [lane("a", Some("north#1"), 1, &["docs/features.md"]), lane("b", Some("north#2"), 2, &["docs/features.md"])];
    assert_eq!(assess(&declared(&["docs/features.md"]), &lanes, &|file| driven.covers(file), Policy::default()), Verdict::Clear);
}

#[test]
fn the_lane_a_session_works_in_is_the_worktree_its_output_names_most_often() {
    let output = r#"{"file_path":"/home/tom/code/wisp/worktrees/perf-scale-185/a.rs"} cd /home/tom/code/wisp/worktrees/perf-scale-185 && ls /home/tom/code/wisp/worktrees/perf-scale-185-base /home/tom/code/wisp/worktrees/g183-msaa/x"#;
    assert_eq!(worktree_in_use(output, "/home/tom/code/wisp/worktrees").as_deref(), Some("perf-scale-185"));
    assert_eq!(worktree_in_use(output, "/home/tom/code/north/worktrees"), None);
    let tie = "/w/a/x /w/b/y";
    assert_eq!(worktree_in_use(tie, "/w/").as_deref(), Some("b"));
}
