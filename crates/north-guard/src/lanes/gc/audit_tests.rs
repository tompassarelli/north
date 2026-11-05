use std::collections::HashSet;

use serde_json::json;

use super::audit::{Findings, Repo, classify_tags, live_ticket, ref_names, render_tags, select_repos, tag_headline};
use super::audit_rows::{Cleanup, Facts, age_text, gc_problems, row, table};

const HOUR: i64 = 3_600_000;
const NOW: i64 = 1_000 * HOUR;

fn names(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

fn facts(branch: &str, age_h: i64) -> Facts<'_> {
    Facts { repo: "north", branch, committed_ms: NOW - age_h * HOUR, landed: false, landing_ref: false, session: None, ticket: false }
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
fn a_branch_is_explained_by_its_worker_ticket_landing_or_idle_deadline() {
    let worker = row(&Facts { session: Some("w-1".into()), ..facts("lane", 30) }, NOW);
    assert_eq!((worker.owner.as_str(), worker.why, &worker.cleanup), ("session w-1", "running worker", &Cleanup::WhenOwnerEnds("w-1".into())));
    assert_eq!(worker.overdue_ms(NOW), None);
    let queued = row(&Facts { ticket: true, landing_ref: true, ..facts("claude/land-a", 1) }, NOW);
    assert_eq!((queued.owner.as_str(), queued.why, &queued.cleanup), ("land ticket", "queued lane", &Cleanup::WhenLanded));
    let done = row(&Facts { landed: true, ..facts("old", 2) }, NOW);
    assert_eq!((done.why, &done.cleanup, done.overdue_ms(NOW)), ("landed, not deleted", &Cleanup::Now, Some(2 * HOUR)));
    let idle = row(&facts("topic", 10), NOW);
    assert_eq!((idle.owner.as_str(), idle.why, &idle.cleanup), ("none", "unlanded, no owner", &Cleanup::At(NOW + 14 * HOUR)));
    assert_eq!(idle.overdue_ms(NOW), None);
    let late = row(&facts("topic", 30), NOW);
    assert_eq!(late.overdue_ms(NOW), Some(6 * HOUR));
    let orphan = row(&Facts { landing_ref: true, ..facts("claude/land-b", 6) }, NOW);
    assert_eq!(orphan.overdue_ms(NOW), Some(HOUR));
}

#[test]
fn the_table_has_plain_columns_sorted_oldest_first() {
    let rows = vec![row(&facts("young", 2), NOW), row(&Facts { landed: true, ..facts("old", 50) }, NOW), row(&facts("middle", 30), NOW)];
    let lines = table(&rows, NOW, &|ms| format!("t{}", ms / HOUR));
    assert_eq!(lines[0], "REPO   BRANCH  AGE    OWNER  WHY IT EXISTS        CLEANED UP");
    assert_eq!(lines[1], "north  old     2d 2h  none   landed, not deleted  now");
    assert_eq!(lines[2], "north  middle  1d 6h  none   unlanded, no owner   overdue since t994");
    assert_eq!(lines[3], "north  young   2h 0m  none   unlanded, no owner   t1022");
}

#[test]
fn age_text_uses_the_two_largest_units() {
    assert_eq!((age_text(5 * 60_000).as_str(), age_text(90 * 60_000).as_str(), age_text(26 * HOUR).as_str()), ("5m", "1h 30m", "1d 2h"));
}

#[test]
fn lane_gc_problems_name_a_failed_unit_a_silent_state_and_branches_stuck_past_a_day() {
    let stuck = vec![row(&facts("topic", 60), NOW)];
    let calm = vec![row(&facts("topic", 30), NOW)];
    let ok = [("lane-gc.service", "success".to_string())];
    assert!(gc_problems(&ok, Some(HOUR), &calm, NOW).is_empty());
    assert_eq!(gc_problems(&[("lane-gc-remote.service", "exit-code".to_string())], Some(HOUR), &calm, NOW), names(&["lane-gc-remote.service last result: exit-code"]));
    assert_eq!(gc_problems(&ok, Some(30 * HOUR), &calm, NOW), names(&["lane-gc has not run for 1d 6h"]));
    assert_eq!(gc_problems(&ok, None, &calm, NOW), names(&["lane-gc has no state file"]));
    assert_eq!(gc_problems(&ok, Some(HOUR), &stuck, NOW), names(&["lane-gc retired nothing for a day while 1 branches are past their cleanup time"]));
}
