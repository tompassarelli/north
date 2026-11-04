use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use serde_json::json;

use super::lander::{self, Facts};
use super::render::format_row;
use super::stall::{Verdict, with_lander};
use crate::foundation::clock::Timestamp;

fn at(text: &str) -> f64 {
    Timestamp::parse_python_datetime(text).expect("timestamp").unix_seconds_f64()
}

fn touch(path: &Path, content: &str, modified: f64) {
    fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    fs::write(path, content).expect("file");
    OpenOptions::new().write(true).open(path).expect("open").set_modified(UNIX_EPOCH + Duration::from_secs_f64(modified)).expect("mtime");
}

fn micros(seconds: f64) -> u64 {
    (seconds * 1e6) as u64
}

fn tonight(directory: &Path) {
    let landed = at("2026-11-04T17:34:00Z");
    touch(&directory.join("done/1793813640000000-earlier.json"), &json!({"verdict": "landed abc"}).to_string(), landed);
    touch(&directory.join("done/1793813000000000-refused.json"), &json!({"verdict": "failed x"}).to_string(), landed + 600.0);
    for lane in 0..12 {
        let queued = landed + 60.0 * f64::from(lane + 1);
        touch(&directory.join(format!("{}-lane-{lane}.json", micros(queued))), "{}", queued);
    }
    for run in 0..10 {
        let started = landed - 4.0 * 3600.0 + f64::from(run) * 1200.0;
        touch(&directory.join(format!("logs/{}-candidate.log", micros(started))), "safe-push: verdict green\n", started + 8.0 * 60.0);
    }
    let running = at("2026-11-04T19:00:00Z");
    touch(&directory.join(format!("logs/{}-candidate.log", micros(running))), "running\n", running + 30.0);
}

fn reasons(directory: &Path, now: &str) -> Vec<String> {
    lander::judge(&lander::read(directory, at(now))).reasons
}

#[test]
fn replaying_12_34_to_2_30_am_alerts_once_main_has_idled_past_twenty_minutes_with_tickets_queued() {
    let root = tempfile::tempdir().expect("temp dir");
    tonight(root.path());
    assert_eq!(reasons(root.path(), "2026-11-04T17:50:00Z"), Vec::<String>::new());
    let idle = reasons(root.path(), "2026-11-04T17:56:00Z");
    assert_eq!(idle.len(), 1);
    assert!(idle[0].starts_with("lander tickets queued: 12 waiting, oldest 21m, and main has not moved through the lander for 22m > 20m"), "{}", idle[0]);
    let late = reasons(root.path(), "2026-11-04T19:30:00Z");
    assert_eq!(late.len(), 2, "{late:?}");
    assert!(late[0].contains("12 waiting") && late[0].contains("for 116m"), "{}", late[0]);
    assert!(late[1].starts_with("lander candidate check running 30m > 16m (2 x its p90)"), "{}", late[1]);
}

#[test]
fn the_check_limit_needs_five_finished_checks_and_an_empty_queue_never_alerts() {
    let root = tempfile::tempdir().expect("temp dir");
    let running = at("2026-11-04T19:00:00Z");
    touch(&root.path().join(format!("logs/{}-candidate.log", micros(running))), "running\n", running);
    assert!(reasons(root.path(), "2026-11-04T21:00:00Z").is_empty());
    touch(&root.path().join("done/1-landed.json"), &json!({"verdict": "landed abc"}).to_string(), running - 7200.0);
    assert!(reasons(root.path(), "2026-11-04T21:00:00Z").is_empty());
}

#[test]
fn a_ticket_in_the_outbox_counts_and_a_missing_directory_reads_as_empty() {
    let root = tempfile::tempdir().expect("temp dir");
    assert_eq!(lander::read(&root.path().join("none"), 10.0), Facts { now: 10.0, ..Facts::default() });
    touch(&root.path().join("outbox/5000000-lane.json"), "{}", 5.0);
    let facts = lander::read(root.path(), 10_000.0);
    assert_eq!((facts.depth, facts.oldest_queued_at), (1, Some(5.0)));
    assert_eq!(lander::judge(&facts).reasons.len(), 1);
}

#[test]
fn the_lander_reasons_make_a_healthy_github_verdict_stalled_and_show_in_the_row() {
    let healthy: Verdict = serde_json::from_value(json!({"stalled": false, "reason": "healthy", "main_age_minutes": 1.0, "main_limit_minutes": 30.0, "median_gap_minutes": null, "oldest_pending_minutes": null, "overdue_jobs": [], "runner": null})).expect("verdict");
    let reasons = vec!["lander tickets queued: 12 waiting".to_owned()];
    let stalled = with_lander(healthy.clone(), &reasons);
    assert!(stalled.stalled && stalled.reason == "lander tickets queued: 12 waiting");
    assert_eq!(with_lander(healthy, &[]).reason, "healthy");
    let row = json!({"repo": "north", "pending": 12, "landed_per_hour": 0, "push_to_land_minutes": 0, "push_to_land_p90_minutes": 0, "lander": {"depth": 12, "oldest_minutes": 116.0, "last_landing_minutes": 116.0, "check_minutes": 30.0}});
    assert!(format_row(&row, None).contains(", lander 12 queued oldest 116m last landing 116m ago check 30m"), "{}", format_row(&row, None));
}

#[test]
fn a_target_date_already_past_is_reported_with_its_owner_not_as_a_projected_miss() {
    let row = json!({"repo": "makgora", "pending": 0, "landed_per_hour": 1, "push_to_land_minutes": 0, "push_to_land_p90_minutes": 0, "epoch": at("2026-11-05T01:00:00Z"), "target": at("2026-10-16T00:00:00Z"), "goal_finish": at("2026-10-20T00:00:00Z")});
    let line = format_row(&row, Some(&[]));
    assert!(line.contains(", target 2026-10-16T00:00:00Z has passed: the [projects.makgora] target in the operator policy needs a new date or removal"), "{line}");
    assert!(!line.contains("projected miss"), "{line}");
}
