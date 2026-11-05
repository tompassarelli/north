#![cfg(test)]

use super::unlanded_work::{Scope, container, stale_lanes_notice};
use std::path::Path;

const REPORT: &str = "# header\n\
78h\tnorth\t~/code/north/worktrees/old-a\told-a\t+3\t-\tA\t#523 wisp#9\tlost\n\
30h\tnorth\t(branch)\tnorth-523-b\t+1\t-\tB\ttompassarelli/north#523\tlost\n\
26h\tnorth\t~/code/north/worktrees/old-c\told-c\t+1\t-\tC\tnorth#523\tlost\n\
25h\tnorth\t~/code/north/worktrees/other\tother\t+1\t-\tD\t#5230 north#52\tlost\n\
23h\tnorth\t~/code/north/worktrees/fresh\tfresh\t+1\t-\tE\t#523\tlost\n\
900h\tmakgora\t~/code/makgora/worktrees/m\tm\t+1\t-\tF\t#523\tlost\n\
900h\twisp\t~/code/wisp/worktrees/w\tw\t+1\t-\tG\t#9\tlost\n";

#[test]
fn a_north_session_hears_only_about_stale_north_lanes_and_those_naming_its_item() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().to_str().unwrap();
    let report = dir.path().join("unlanded.txt");
    std::fs::write(&report, REPORT).unwrap();
    let cwd = dir.path().join("code/north/main");
    let scope = Scope { home, container: container(&cwd, home), item: Some(("north", 523)) };
    let notice = stale_lanes_notice(&report, &scope).unwrap();
    assert_eq!(
        notice,
        "4 unlanded north lanes older than 24 h are listed in ~/unlanded.txt (north lane-gc --unlanded). Before starting an issue, continue an existing lane that references it instead of starting over.\nLanes referencing north#523: ~/code/north/worktrees/old-a, north-523-b and 1 more.\n"
    );
    assert!(notice.len() < 400, "{} chars", notice.len());
    assert!(!notice.contains("makgora") && !notice.contains("wisp"), "{notice}");
    let unscoped = Scope { home, container: container(&cwd, home), item: None };
    assert!(!stale_lanes_notice(&report, &unscoped).unwrap().contains("Lanes referencing"));
}

#[test]
fn a_session_outside_a_code_container_or_with_no_stale_lane_gets_no_notice() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().to_str().unwrap();
    let report = dir.path().join("unlanded.txt");
    std::fs::write(&report, REPORT).unwrap();
    assert_eq!(container(Path::new("/tmp/lp-briefs"), home), None);
    assert_eq!(container(&dir.path().join("code/wisp/worktrees/x/src"), home), Some("wisp"));
    let at = |container| Scope { home, container, item: None };
    assert_eq!(stale_lanes_notice(&report, &at(None)), None);
    assert_eq!(stale_lanes_notice(&report, &at(Some("south"))), None);
    assert!(stale_lanes_notice(&report, &at(Some("wisp"))).unwrap().starts_with("1 unlanded wisp lanes"));
    assert_eq!(stale_lanes_notice(&dir.path().join("missing"), &at(Some("north"))), None);
}
