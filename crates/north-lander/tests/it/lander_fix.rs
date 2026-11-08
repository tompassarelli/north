use std::fs;

use north_lander::land::queue;

use super::serve::{Repository, World, git, report};

const URGENT_AHEAD: [(&str, &str, bool); 7] = [
    ("red-fix-a", "2026-11-08T04:01:12Z", true),
    ("red-fix-b", "2026-11-08T04:05:40Z", true),
    ("red-fix-c", "2026-11-08T04:09:03Z", true),
    ("urgent-a", "2026-11-08T04:12:51Z", false),
    ("urgent-b", "2026-11-08T04:18:27Z", false),
    ("urgent-c", "2026-11-08T04:22:09Z", false),
    ("urgent-d", "2026-11-08T04:30:02Z", false),
];

const PLAIN_AHEAD: (&str, &str) = ("n909-redlanes", "2026-11-08T04:26:35Z");

const LANDER_FIX: (&str, &str) = ("q902-deadcode", "2026-11-08T04:37:14Z");

const BEHIND: [(&str, &str); 12] = [
    ("g917-edges", "2026-11-08T04:37:46Z"),
    ("s78-abs-paths-case", "2026-11-08T04:39:11Z"),
    ("n842-classify", "2026-11-08T04:46:34Z"),
    ("n842-onejob", "2026-11-08T04:49:20Z"),
    ("lane-a", "2026-11-08T04:55:49Z"),
    ("lane-b", "2026-11-08T05:01:17Z"),
    ("lane-c", "2026-11-08T05:07:30Z"),
    ("lane-d", "2026-11-08T05:12:44Z"),
    ("lane-e", "2026-11-08T05:19:05Z"),
    ("lane-f", "2026-11-08T05:26:38Z"),
    ("lane-g", "2026-11-08T05:33:21Z"),
    ("lane-h", "2026-11-08T05:50:13Z"),
];

fn micros(rfc3339: &str) -> u128 {
    let clock: Vec<u8> = rfc3339[11..19].split(':').map(|part| part.parse().unwrap()).collect();
    let at = time::Date::from_calendar_date(2026, time::Month::November, 8).unwrap().with_hms(clock[0], clock[1], clock[2]).unwrap().assume_utc();
    u128::try_from(at.unix_timestamp_nanos() / 1000).unwrap()
}

fn queue_as_it_stood(repository: &Repository, branch: &str, queued_at: &str, marks: &[(&str, serde_json::Value)]) {
    repository.lane(branch);
    let suffix = format!("-{}.json", queue::slug(branch));
    let written = fs::read_dir(&repository.queue).unwrap().flatten().map(|entry| entry.path()).find(|path| path.to_string_lossy().ends_with(&suffix)).unwrap();
    let mut ticket: serde_json::Value = serde_json::from_str(&fs::read_to_string(&written).unwrap()).unwrap();
    let queued = micros(queued_at);
    ticket["queued_at"] = queued_at.into();
    ticket["queued_micros"] = serde_json::json!(queued);
    for (field, value) in marks {
        ticket[*field] = value.clone();
    }
    fs::remove_file(&written).unwrap();
    fs::write(repository.queue.join(queue::file_name(queued, branch)), serde_json::to_string_pretty(&ticket).unwrap()).unwrap();
}

#[test]
fn the_lander_fix_queued_at_12_57_pm_behind_twenty_lanes_and_seven_urgent_ones_is_the_first_commit_of_the_one_push_of_the_whole_queue() {
    let world = World::new();
    let repository = world.repository("north");
    let start = git(&repository.checkout, &["rev-parse", "HEAD"]);
    for (branch, queued_at, red_fix) in URGENT_AHEAD {
        let mark = if red_fix { ("red_fix", serde_json::json!("marker")) } else { ("urgent", serde_json::json!("proxy order 2026-11-08")) };
        queue_as_it_stood(&repository, branch, queued_at, &[mark]);
    }
    queue_as_it_stood(&repository, PLAIN_AHEAD.0, PLAIN_AHEAD.1, &[]);
    queue_as_it_stood(&repository, LANDER_FIX.0, LANDER_FIX.1, &[("urgent", serde_json::json!("land-first")), ("lander_fix", serde_json::json!("land-first removes the probe rounds"))]);
    for (branch, queued_at) in BEHIND {
        queue_as_it_stood(&repository, branch, queued_at, &[]);
    }

    let output = world.lander(&repository, None).wait_with_output().unwrap();
    let transcript = report(&output);

    git(&repository.checkout, &["fetch", "-q", "origin"]);
    let landed = git(&repository.checkout, &["log", "--first-parent", "--reverse", "--format=%P %s", &format!("{start}..origin/main")]);
    assert_eq!(landed.lines().count(), URGENT_AHEAD.len() + 2 + BEHIND.len(), "the whole queue lands in one push, the lander fix with it\n{landed}\n{transcript}");
    let first = landed.lines().next().unwrap_or_default();
    assert!(first.starts_with(&format!("{start} {} ", LANDER_FIX.0)), "the lander fix is the first commit of the push, onto the 12:57 PM main, so no lane queued ahead of it can conflict it out: {first}\n{landed}\n{transcript}");
}
