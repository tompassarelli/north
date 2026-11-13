use std::fs;
use std::path::Path;

use rustix::fs::{FlockOperation, flock};

use super::{Ask, Event, QUEUE, RECORD, record, wait};

const NOW: u64 = 1_763_000_000;
const CHECK: &str = "cargo nextest run -p north-capacity";

fn name(rank: u8, queued_at: u64, pid: u32, checkout: &str) -> String {
    format!("{rank}-{}-{pid}@{checkout}", u128::from(queued_at) * 1_000_000_000)
}

fn stamp(root: &Path, kind: &'static str, ticket: &str, at: u64) {
    let event = Event { kind, ticket: ticket.to_owned(), rank: 2, check: CHECK.to_owned(), class: "land", exit: (kind == "ended").then_some(0), deadline: None };
    assert_eq!(record(root, &event, at), 0);
}

fn hold(root: &Path, ticket: &str) -> fs::File {
    let file = fs::File::create(root.join(QUEUE).join(ticket)).unwrap();
    flock(&file, FlockOperation::LockExclusive).unwrap();
    file
}

fn ask<'a>(rank: u8, ticket: Option<&'a str>, checkout: Option<&'a str>) -> Ask<'a> {
    Ask { rank, slots: None, ticket, checkout, host_cap: None, within: None }
}

#[test]
fn line_wait_verb_refuses_an_ordinary_check_past_its_deadline_or_within_and_admits_when_off_or_unreadable() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    fs::create_dir_all(root.join(QUEUE)).unwrap();
    for lock in ["c.lock", "c-1.lock", "c.background.lock", "d.lock", "d-1.lock", "warm.lock"] {
        fs::write(root.join(lock), "").unwrap();
    }
    for pid in 0..3 {
        let ticket = name(2, NOW - 4000, pid, "d");
        stamp(root, "started", &ticket, NOW - 3600);
        stamp(root, "ended", &ticket, NOW - 3300);
    }
    for pid in 10..12 {
        stamp(root, "started", &name(2, NOW - 100, pid, "c"), NOW - 60);
    }
    let own = name(3, NOW - 50, 30, "c");
    let mut held: Vec<fs::File> = (20..24).map(|pid| hold(root, &name(2, NOW - 40, pid, "c"))).collect();
    held.push(hold(root, &own));
    held.push(hold(root, &name(0, NOW - 40, 40, "d")));

    assert_eq!(wait(root, &ask(3, Some(&own), Some("c")), true, NOW), (96, "expected wait 840 s, over the rank's deadline 600 s".to_owned()));
    assert_eq!(wait(root, &ask(0, None, Some("c")), true, NOW), (0, "expected wait 240 s".to_owned()));
    let roomy = |within| Ask { slots: Some(4), within, ..ask(3, Some(&own), Some("c")) };
    assert_eq!(wait(root, &roomy(None), true, NOW), (0, "expected wait 300 s".to_owned()));
    assert_eq!(wait(root, &roomy(Some(200)), true, NOW), (96, "expected wait 300 s, over the rank's deadline 200 s".to_owned()));
    assert_eq!(wait(root, &Ask { within: Some(900), ..ask(3, Some(&own), Some("c")) }, true, NOW), (96, "expected wait 840 s, over the rank's deadline 600 s".to_owned()));
    assert_eq!(wait(root, &ask(3, Some(&own), Some("c")), false, NOW), (0, "expected wait 0 s".to_owned()));
    fs::write(root.join(RECORD), [0xff, 0xfe, 0x00, 0x9f]).unwrap();
    assert_eq!(wait(root, &ask(3, Some(&own), Some("c")), true, NOW), (0, "expected wait 0 s".to_owned()));
    drop(held);
}
