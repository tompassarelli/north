use std::collections::BTreeMap;
use std::fs;

use serde_json::{Value, json};

use super::core::{Alert, Entry, REPEAT, Rows, State, duration, presence, read_registry, step};
use crate::foundation::clock::Timestamp;

const T: f64 = 1_800_000_000.0;

fn iso(time: f64) -> Value {
    Value::String(Timestamp::from_unix_seconds_f64(time).iso())
}

fn entries() -> Vec<Entry> {
    let entry = |name: &str, job: &str, placement: &str, deadline, period| Entry {
        name: name.into(),
        job: job.into(),
        placement: placement.into(),
        deadline,
        period: Some(period),
    };
    vec![entry("nightly-backup", "nightly-backup", "laptop", 1800, 86400), entry("sync@laptop", "sync", "laptop", 7200, 900), entry("checks", "checks", "nexus", 3600, 900)]
}

fn rows(now: f64, backup_ok: f64) -> Rows {
    let row = |last_ok: f64, open: Option<f64>| json!({"first_started": iso(T - 86400.0), "last_ok": iso(last_ok), "open_since": open.map_or(Value::Null, iso)});
    BTreeMap::from([
        (("nightly-backup".into(), "laptop".into()), row(backup_ok, None)),
        (("sync".into(), "laptop".into()), row(T - 3600.0, Some(T - 3600.0 + 60.0))),
        (("checks".into(), "nexus".into()), row(now - 600.0, None)),
    ])
}

fn hosts() -> BTreeMap<String, String> {
    BTreeMap::from([("laptop".into(), "laptop".into()), ("nexus".into(), "nexus".into())])
}

fn alert_texts(alerts: &[Alert]) -> Vec<(String, String, String)> {
    alerts.iter().map(|alert| (alert.name.clone(), alert.title.clone(), alert.text.clone())).collect()
}

fn run_scenario() -> Vec<Vec<(String, String, String)>> {
    let off = json!({"peers": {"laptop": {"handshake": T - 3600.0, "online_since": null}}});
    let on = |now: f64| json!({"peers": {"laptop": {"handshake": now - 60.0, "online_since": T + 600.0}}});
    let steps = [
        (T, rows(T, T - 7200.0), off),
        (T + 3600.0, rows(T + 3600.0, T - 7200.0), on(T + 3600.0)),
        (T + 4500.0, rows(T + 4500.0, T + 3900.0), on(T + 4500.0)),
        (T + 5400.0, rows(T + 5400.0, T + 3900.0), on(T + 5400.0)),
        (T + 5400.0 + REPEAT, rows(T + 5400.0 + REPEAT, T + 3900.0 + REPEAT), on(T + 5400.0 + REPEAT)),
    ];
    let mut state = State::new();
    let mut seen = Vec::new();
    for (now, rows, peers) in steps {
        let (alerts, next) = step(&entries(), &rows, &hosts(), &presence(&peers, "laptop", now), &state, now);
        seen.push(alert_texts(&alerts));
        state = next;
    }
    seen
}

fn one(name: &str, title: &str, text: &str) -> Vec<(String, String, String)> {
    vec![(name.into(), title.into(), text.into())]
}

#[test]
fn overdue_laptop_jobs_alert_once_recover_and_repeat_after_a_day_with_the_recorded_texts() {
    assert_eq!(
        run_scenario(),
        vec![
            one(
                "nightly-backup",
                "Nexus: nightly-backup overdue",
                "nightly-backup missed its deadline; laptop offline since 2027-01-15 07:00 UTC; it runs on wake"
            ),
            one(
                "sync@laptop",
                "Nexus: sync overdue",
                "sync has had no successful run for 45min while the laptop has been online since 2027-01-15 08:10 UTC; last success 2027-01-15 07:00 UTC"
            ),
            one("nightly-backup", "Nexus: nightly-backup recovered", "nightly-backup recovered; last success 2027-01-15 09:05 UTC"),
            vec![],
            one(
                "sync@laptop",
                "Nexus: sync overdue",
                "sync missed its deadline (2h); laptop online since 2027-01-15 08:10 UTC; last success 2027-01-15 07:00 UTC; its run started 2027-01-15 07:01 UTC never finished"
            ),
        ]
    );
}

fn a_nexus_job_past_its_deadline_names_nexus_and_ignores_laptop_presence() {
    let late = json!({"first_started": iso(T - 86400.0), "last_ok": iso(T - 7200.0), "open_since": null});
    let rows = BTreeMap::from([(("checks".to_owned(), "nexus".to_owned()), late)]);
    let laptop = presence(&json!({}), "laptop", T);
    let (alerts, state) = step(&entries(), &rows, &hosts(), &laptop, &State::new(), T);
    assert_eq!(
        alert_texts(&alerts),
        one("checks", "Nexus: checks overdue", "checks on nexus missed its deadline (1h); last success 2027-01-15 06:00 UTC")
    );
    assert_eq!(state["checks"], json!({"overdue": true, "alerted": T}));
}

fn a_job_that_never_succeeded_counts_from_its_first_start() {
    let never = json!({"first_started": iso(T - 3.0 * 3600.0), "last_ok": null, "open_since": null});
    let rows = BTreeMap::from([(("checks".to_owned(), "nexus".to_owned()), never)]);
    let (alerts, _) = step(&entries(), &rows, &hosts(), &presence(&json!({}), "laptop", T), &State::new(), T);
    assert_eq!(alerts.len(), 1);
    assert!(alerts[0].text.ends_with("last success never"), "{}", alerts[0].text);
}

fn a_job_without_ledger_rows_keeps_its_prior_state_and_alerts_nothing() {
    let prior = State::from([("checks".to_owned(), json!({"overdue": true, "alerted": T - 10.0}))]);
    let (alerts, next) = step(&entries(), &Rows::new(), &hosts(), &presence(&json!({}), "laptop", T), &prior, T);
    assert!(alerts.is_empty());
    assert_eq!(next, prior);
}

fn laptop_presence_needs_a_handshake_within_five_minutes() {
    let fresh = presence(&json!({"peers": {"laptop": {"handshake": T - 60.0, "online_since": T - 5000.0}}}), "laptop", T);
    assert_eq!((fresh.handshake, fresh.online_since), (Some(T - 60.0), Some(T - 5000.0)));
    let stale = presence(&json!({"peers": {"laptop": {"handshake": T - 600.0, "online_since": T - 5000.0}}}), "laptop", T);
    assert_eq!((stale.handshake, stale.online_since), (Some(T - 600.0), None));
    let missing = presence(&Value::Null, "laptop", T);
    assert_eq!((missing.handshake, missing.online_since), (None, None));
}

fn durations_accept_the_registry_units_and_reject_anything_else() {
    let parsed: Vec<_> = ["30min", "2h", " 90 m ", "8d", "1w", "45s", "soon", "2 weeks", ""].map(duration).into();
    assert_eq!(parsed, vec![Some(1800), Some(7200), Some(5400), Some(691_200), Some(604_800), Some(45), None, None, None]);
}

fn the_registry_keeps_only_placed_routines_with_a_deadline_and_derives_their_period() {
    let dir = tempfile::tempdir().unwrap();
    let write = |file: &str, text: &str| fs::write(dir.path().join(file), text).unwrap();
    write("a.md", "---\nname: nightly-backup\nplacement: laptop\ndeadline: 30min\nschedule: daily at 03:00\n---\nbody\n");
    write("b.md", "---\nname: sync@laptop\nplacement: laptop\ndeadline: 2h\nschedule: every 15 min\n---\n");
    write("c.md", "---\nplacement: nexus\ndeadline: 90m\nschedule: hourly\n---\n");
    write("d.md", "no frontmatter\n");
    write("e.md", "---\nname: elsewhere\nplacement: cloud\ndeadline: 1h\n---\n");
    write("f.md", "---\nname: baddeadline\nplacement: nexus\ndeadline: soon\n---\n");
    write("g.md", "---\nname: weekly-sweep@laptop\nplacement: laptop\ndeadline: 8d\nschedule: weekly on sunday\n---\n");
    write("h.txt", "---\nname: wrongext\nplacement: nexus\ndeadline: 1h\n---\n");
    let seen: Vec<_> = read_registry(dir.path()).into_iter().map(|e| (e.name, e.job, e.placement, e.deadline, e.period)).collect();
    let expected = [
        ("nightly-backup", "nightly-backup", "laptop", 1800, Some(86400)),
        ("sync@laptop", "sync", "laptop", 7200, Some(900)),
        ("c", "c", "nexus", 5400, Some(3600)),
        ("weekly-sweep@laptop", "weekly-sweep", "laptop", 691_200, Some(604_800)),
    ];
    let expected: Vec<_> = expected.iter().map(|(a, b, c, d, e)| (a.to_string(), b.to_string(), c.to_string(), *d, *e)).collect();
    assert_eq!(seen, expected);
}

#[test]
fn nexus_jobs_presence_durations_and_the_registry_follow_the_python_rules() {
    a_nexus_job_past_its_deadline_names_nexus_and_ignores_laptop_presence();
    a_job_that_never_succeeded_counts_from_its_first_start();
    a_job_without_ledger_rows_keeps_its_prior_state_and_alerts_nothing();
    laptop_presence_needs_a_handshake_within_five_minutes();
    durations_accept_the_registry_units_and_reject_anything_else();
    the_registry_keeps_only_placed_routines_with_a_deadline_and_derives_their_period();
}
