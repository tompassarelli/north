use std::fs;
use std::path::Path;

use super::*;

fn entry(name: &str, kind: &str, placement: &str, extra: &str) -> String {
    format!("---\nname: {name}\nkind: {kind}\nschedule: daily at 03:00\nowner: tom\npurpose: keep it\nrelates-to: [north#1, south#2]\nexpires: never\nplacement: {placement}\n{extra}---\nRun the thing.\n")
}

fn registry(files: &[(&str, String)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        fs::write(dir.path().join(name), text).unwrap();
    }
    dir
}

fn listed(root: &Path, live: Option<Option<Vec<String>>>, crons: Option<&str>) -> (u8, String) {
    list(root, &Listing { live, crons: crons.map(str::to_owned), host: "laptop".to_owned() }).unwrap()
}

#[test]
fn a_complete_entry_parses_its_fields_list_and_body() {
    let parsed = parse(&entry("backup", "systemd-timer", "any", "deadline: 2d\n"), "backup").unwrap();
    assert_eq!(parsed.relates_to, ["north#1", "south#2"]);
    assert_eq!(parsed.body, "Run the thing.");
    assert_eq!(show(&parsed), "# routine backup (systemd-timer, daily at 03:00, placed on any, deadline 2d): keep it\nRun the thing.");
    assert_eq!(pointer("backup"), "[routine:backup] Run `agents routines show backup` and follow it.");
}

#[test]
fn each_invalid_entry_names_its_first_problem() {
    let cases = [
        ("plain text".to_owned(), "x", "no front matter"),
        ("---\nname: x\n---\nbody\n".to_owned(), "x", "missing kind, schedule, owner, purpose, relates-to, expires, placement"),
        (entry("other", "systemd-timer", "any", ""), "x", "name 'other' differs from file 'x'"),
        (entry("x", "cron", "any", ""), "x", "kind 'cron' is not one of session-cron, systemd-timer, cloud-routine"),
        (entry("x", "systemd-timer", "cloud", ""), "x", "placement 'cloud' is not one of nexus, gex45, laptop, any"),
        (entry("x@moon", "systemd-timer", "nexus", ""), "x@moon", "name suffix @moon is not one of @nexus, @gex45, @laptop"),
        (entry("x@nexus", "systemd-timer", "laptop", ""), "x@nexus", "placement 'laptop' does not match name suffix @nexus"),
        (entry("x", "systemd-timer", "any", "deadline: soon\n"), "x", "deadline 'soon' is not a duration such as 2d, 30min or 1h"),
        (entry("x", "systemd-timer", "any", "").replace("expires: never", "expires: someday"), "x", "expires 'someday' is neither never nor YYYY-MM-DD"),
        (entry("x", "systemd-timer", "any", "").replace("Run the thing.\n", "  \n"), "x", "empty prompt or command"),
    ];
    for (text, stem, problem) in cases {
        assert_eq!(parse(&text, stem).unwrap_err(), problem, "{text}");
    }
}

#[test]
fn listing_flags_invalid_expired_and_unregistered_session_crons() {
    let dir = registry(&[
        ("a-old.md", entry("a-old", "cloud-routine", "any", "").replace("expires: never", "expires: 2020-01-01")),
        ("bad.md", "nothing".to_owned()),
        ("watch.md", entry("watch", "session-cron", "any", "")),
        ("notes.txt", "ignored".to_owned()),
    ]);
    let (code, text) = listed(dir.path(), None, Some("*/5 [routine:watch] go\n\n*/9 [routine:gone] go\nrandom cron\n"));
    assert_eq!(code, 1);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], format!("{:28} {:14} {:34} {:9} OWNER", "NAME", "KIND", "SCHEDULE", "PLACEMENT"));
    assert_eq!(lines[1], format!("{:28} {:14} {:34} {:9} tom", "a-old", "cloud-routine", "daily at 03:00", "any"));
    assert_eq!(
        &lines[3..],
        [
            "session crons: 3 checked",
            "INVALID bad.md: no front matter",
            "EXPIRED a-old: expired 2020-01-01",
            "UNREGISTERED session-cron [routine:gone]: no such session-cron entry",
            "UNREGISTERED session-cron: random cron",
            "routines: 2 entries, 4 problem(s)",
        ]
    );
}

#[test]
fn live_timers_are_matched_to_their_host_entry_or_flagged() {
    let dir = registry(&[
        ("sync@laptop.md", entry("sync@laptop", "systemd-timer", "laptop", "")),
        ("sync@nexus.md", entry("sync@nexus", "systemd-timer", "nexus", "")),
        ("backup.md", entry("backup", "systemd-timer", "nexus", "")),
        ("idle.md", entry("idle", "systemd-timer", "any", "")),
        ("remote.md", entry("remote", "systemd-timer", "nexus", "")),
    ]);
    let timers = vec!["sync".to_owned(), "backup".to_owned(), "stray".to_owned()];
    let (code, text) = listed(dir.path(), Some(Some(timers)), None);
    assert_eq!(code, 1);
    let tail: Vec<&str> = text.lines().skip(6).collect();
    assert_eq!(
        tail,
        [
            "live timers: 3 (2 registered)",
            "not live: idle",
            "elsewhere: remote (placed on nexus)",
            "WRONG HOST: backup runs here but is placed on nexus",
            &format!("UNREGISTERED systemd-timer stray: add {}/stray.md", dir.path().display()),
            "routines: 5 entries, 2 problem(s)",
        ]
    );
    let (_, unavailable) = listed(dir.path(), Some(None), None);
    assert!(unavailable.contains("live timers: systemctl --user unavailable\n"));
    let (_, nexus, _) = placement_findings(dir.path(), &load(dir.path()).unwrap().0, &["sync".to_owned()], "nexus");
    assert_eq!(nexus, ["not live: backup", "not live: idle", "not live: remote"]);
}

#[test]
fn a_clean_registry_exits_zero() {
    let dir = registry(&[("watch.md", entry("watch", "session-cron", "any", ""))]);
    assert_eq!(listed(dir.path(), None, None), (0, format!("{:28} {:14} {:34} {:9} OWNER\n{:28} {:14} {:34} {:9} tom\nroutines: 1 entries, 0 problem(s)\n", "NAME", "KIND", "SCHEDULE", "PLACEMENT", "watch", "session-cron", "daily at 03:00", "any")));
}
