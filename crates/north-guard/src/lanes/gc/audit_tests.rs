use std::collections::{HashMap, HashSet};

use serde_json::json;

use super::audit::{Findings, named_issues, Repo, classify_tags, live_ticket, ref_names, render_tags, select_repos, tag_headline};
use super::audit_rows::{Facts, age_text, fault, gc_problems, leftover_table, open_table, row};
use super::evidence::IssueState;

const HOUR: i64 = 3_600_000;
const NOW: i64 = 1_000 * HOUR;

fn names(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

fn facts(branch: &str, age_h: i64) -> Facts<'_> {
    Facts { repo: "north", branch, committed_ms: NOW - age_h * HOUR, landed: false, session: None, ticket: false, issues: Vec::new() }
}

#[test]
fn repositories_are_unarchived_pushed_this_year_and_not_foreign_forks() {
    let listing = json!([
        {"name": "north", "isArchived": false, "isFork": false, "pushedAt": "2026-11-05T01:00:00Z", "defaultBranchRef": {"name": "main"}, "parent": null},
        {"name": "old", "isArchived": false, "isFork": false, "pushedAt": "2025-12-31T23:59:59Z", "defaultBranchRef": {"name": "main"}, "parent": null},
        {"name": "dead", "isArchived": true, "isFork": false, "pushedAt": "2026-03-01T00:00:00Z", "defaultBranchRef": {"name": "main"}, "parent": null},
        {"name": "codex", "isArchived": false, "isFork": true, "pushedAt": "2026-03-01T00:00:00Z", "defaultBranchRef": {"name": "main"}, "parent": {"owner": {"login": "openai"}}},
        {"name": "mine-fork", "isArchived": false, "isFork": true, "pushedAt": "2026-03-01T00:00:00Z", "defaultBranchRef": {"name": "trunk"}, "parent": {"owner": {"login": "tompassarelli"}}}
    ]);
    let all = select_repos(&listing, &[]);
    assert_eq!(all, vec![Repo { name: "mine-fork".into(), default: "trunk".into() }, Repo { name: "north".into(), default: "main".into() }]);
    assert_eq!(select_repos(&listing, &names(&["north"])).len(), 1);
}

#[test]
fn matching_refs_pages_yield_short_names() {
    let pages = json!([[{"ref": "refs/tags/v1"}, {"ref": "refs/tags/gen-2"}], [{"ref": "refs/tags/v3"}]]);
    assert_eq!(ref_names(&pages, "refs/tags/"), names(&["v1", "gen-2", "v3"]));
}

#[test]
fn only_a_landing_branch_with_a_pull_a_run_a_pending_autoland_or_a_fresh_tip_has_a_live_ticket() {
    assert!(live_ticket("claude/land-a", true, false, false, 99 * HOUR));
    assert!(live_ticket("claude/land-a", false, true, false, 99 * HOUR));
    assert!(live_ticket("claude/land-a", false, false, true, 99 * HOUR));
    assert!(live_ticket("claude/land-a", false, false, false, HOUR));
    assert!(!live_ticket("claude/land-a", false, false, false, 5 * HOUR));
    assert!(!live_ticket("topic", true, true, true, 0));
}

#[test]
fn tags_are_sorted_into_families_strays_and_release_gaps() {
    let tags = names(&["v1.0", "v1.1", "gen-1", "gen-2", "pin-a", "archive/north/x/abc", "latest", "vendor"]);
    let found = classify_tags(&tags, &["v1.0".to_string()].into());
    assert_eq!(found.unreleased, names(&["v1.1"]));
    assert_eq!(found.families["gen-*"], names(&["gen-1", "gen-2"]));
    assert_eq!(found.families["pin-*"], names(&["pin-a"]));
    assert_eq!(found.families["archive/*"], names(&["archive/north/x/abc"]));
    assert_eq!(found.strays, names(&["latest", "vendor"]));
    assert_eq!(tag_headline("firn", &found), "firn: 6 non-release tags, 1 v-tags without release");
    assert_eq!(render_tags("firn", &found)[1], "  non-release tags archive/* (1): archive/north/x/abc");
}

#[test]
fn released_v_tags_alone_are_clean() {
    assert_eq!(classify_tags(&names(&["v1.0"]), &["v1.0".to_string()].into()), Findings::default());
    assert!(classify_tags(&[], &HashSet::new()).clean());
}

#[test]
fn a_branch_is_open_work_unless_its_work_is_on_the_default_branch_without_an_owner() {
    let worker = row(&Facts { session: Some("w-1".into()), landed: true, ..facts("lane", 30) }, NOW);
    assert_eq!((worker.owner.as_str(), worker.leftover, worker.alarm), ("session w-1", false, false));
    let queued = row(&Facts { ticket: true, landed: true, ..facts("claude/land-a", 5) }, NOW);
    assert_eq!((queued.owner.as_str(), queued.leftover), ("land ticket", false));
    let open = row(&facts("topic", 500), NOW);
    assert_eq!((open.owner.as_str(), open.leftover, open.alarm), ("none", false, false));
    let fresh = row(&Facts { landed: true, ..facts("old", 0) }, NOW);
    assert_eq!((fresh.leftover, fresh.alarm), (true, false));
    let stale = row(&Facts { landed: true, ..facts("old", 2) }, NOW);
    assert_eq!((stale.leftover, stale.alarm), (true, true));
}

#[test]
fn open_work_is_grouped_under_its_issue_with_state_owner_and_fault() {
    let states: HashMap<String, IssueState> = [("north#360".to_string(), IssueState::Open), ("north#9".to_string(), IssueState::Closed)].into();
    let rows = vec![
        row(&Facts { issues: names(&["#360"]), session: Some("w-1".into()), ..facts("middle", 30) }, NOW),
        row(&Facts { issues: names(&["#360"]), ..facts("young", 2) }, NOW),
        row(&Facts { issues: names(&["#9"]), ..facts("shipped", 80) }, NOW),
        row(&facts("orphan", 5), NOW),
        row(&Facts { landed: true, ..facts("gone", 99) }, NOW),
    ];
    let lines = open_table(&rows, &states);
    assert_eq!(lines[0], "ISSUE       STATE   REPO   BRANCH   AGE    OWNER        FAULT");
    assert_eq!(lines[1], "north#9     closed  north  shipped  3d 8h  none         all its issues are closed");
    assert_eq!(lines[2], "north#360   open    north  middle   1d 6h  session w-1");
    assert_eq!(lines[3], "                    north  young    2h 0m  none");
    assert_eq!(lines[4], "(no issue)  -       north  orphan   5h 0m  none         names no issue");
    assert_eq!(lines.len(), 5);
}

#[test]
fn faults_are_a_stale_landed_branch_closed_issues_or_no_issue_unless_someone_owns_the_branch() {
    let states: HashMap<String, IssueState> = [("north#1".to_string(), IssueState::Open), ("north#2".to_string(), IssueState::Closed)].into();
    let with = |issues: &[&str], age: i64| row(&Facts { issues: names(issues), ..facts("b", age) }, NOW);
    assert_eq!(fault(&with(&["#1"], 500), &states), None);
    assert_eq!(fault(&with(&["#2"], 1), &states), Some("all its issues are closed"));
    assert_eq!(fault(&with(&["#1", "#2"], 1), &states), None);
    assert_eq!(fault(&with(&[], 1), &states), Some("names no issue"));
    assert_eq!(fault(&with(&["#404"], 1), &states), None);
    let owned = row(&Facts { ticket: true, ..facts("claude/land-x", 1) }, NOW);
    assert_eq!(fault(&owned, &states), None);
    assert_eq!(fault(&row(&Facts { landed: true, ..facts("b", 2) }, NOW), &states), Some("landed, still present"));
    assert_eq!(fault(&row(&Facts { landed: true, ..facts("b", 0) }, NOW), &states), None);
}

#[test]
fn issues_come_from_ref_lines_and_item_lines_once_each() {
    let text = "Add the verb\n\nRefs north#360\nItem: north#360\n\nItem: firn#9\nFixes #7\n";
    assert_eq!(named_issues(text), names(&["north#360", "#7", "firn#9"]));
    assert!(named_issues("no issue here").is_empty());
}

#[test]
fn landed_leftovers_get_their_own_table_oldest_first() {
    let rows = vec![row(&Facts { landed: true, ..facts("b", 3) }, NOW), row(&Facts { landed: true, ..facts("a", 50) }, NOW), row(&facts("open", 9), NOW)];
    assert_eq!(leftover_table(&rows), names(&["REPO   BRANCH  AGE    OWNER", "north  a       2d 2h  none", "north  b       3h 0m  none"]));
}

#[test]
fn age_text_uses_the_two_largest_units() {
    assert_eq!((age_text(5 * 60_000).as_str(), age_text(90 * 60_000).as_str(), age_text(26 * HOUR).as_str()), ("5m", "1h 30m", "1d 2h"));
}

#[test]
fn lane_gc_problems_name_a_failed_unit_a_silent_state_and_landed_branches_stuck_past_a_day() {
    let stuck = vec![row(&Facts { landed: true, ..facts("topic", 60) }, NOW)];
    let calm = vec![row(&Facts { landed: true, ..facts("topic", 30) }, NOW), row(&facts("open", 900), NOW)];
    let ok = [("lane-gc.service", "success".to_string())];
    let recent = vec![row(&Facts { landed: true, ..facts("topic", 3) }, NOW)];
    assert!(gc_problems(&ok, Some(HOUR), &recent).is_empty());
    assert_eq!(gc_problems(&[("lane-gc-remote.service", "exit-code".to_string())], Some(HOUR), &recent), names(&["lane-gc-remote.service last result: exit-code"]));
    assert_eq!(gc_problems(&ok, Some(30 * HOUR), &recent), names(&["lane-gc has not run for 1d 6h"]));
    assert_eq!(gc_problems(&ok, None, &recent), names(&["lane-gc has no state file"]));
    assert_eq!(gc_problems(&ok, Some(HOUR), &stuck), names(&["lane-gc retired nothing for a day while 1 landed branches still exist"]));
    assert_eq!(gc_problems(&ok, Some(HOUR), &calm), names(&["lane-gc retired nothing for a day while 1 landed branches still exist"]));
}
