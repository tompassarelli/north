use std::cell::Cell;

use super::quiet_phrase::{Phrase, Table, classify, decide, lease_minutes};
use crate::capacity::mode::{self, Limits, Mode};

const NOW: u64 = 1_800_000_000;

#[test]
fn the_table_reads_quiet_on_and_off_phrases_and_nothing_else() {
    let table = Table::bundled();
    assert_eq!(classify("I'm going to test the wisp map now", &table), Some(Phrase::On("test wisp".into())));
    assert_eq!(classify("playtest makgora with me", &table), Some(Phrase::On("playtest makgora".into())));
    assert_eq!(classify("done testing", &table), Some(Phrase::Off("done testing".into())));
    assert_eq!(classify("OK, finished playing. Release the machine", &table), Some(Phrase::Off("release the machine".into())));
    for quiet in [
        "write a test for the map loader",
        "the brief says \"I'm going to test the wisp map now\" turns quiet mode on",
        "<task-notification>test the wisp map</task-notification>",
        "is the build done?",
        &format!("{} test the wisp map", "word ".repeat(40)),
    ] {
        assert_eq!(classify(quiet, &table), None, "{quiet}");
    }
}

#[test]
fn the_hook_turns_quiet_mode_on_and_off_and_prints_one_line_only_when_it_fires() {
    let (table, limits) = (Table::bundled(), Limits::bundled());
    let dir = tempfile::tempdir().unwrap();
    let closed = Cell::new(0);
    let teardown = || {
        closed.set(closed.get() + 1);
        "closed Warcraft III (1 processes)".to_owned()
    };
    let on = decide("I'm going to test the wisp map now", &table, &limits, dir.path(), NOW, teardown).unwrap();
    assert_eq!(on.lines().count(), 1);
    assert!(on.starts_with("capacity mode: playtest lease from \"test wisp\" until "), "{on}");
    let held = mode::read(dir.path());
    assert_eq!((held.mode, held.hold.map(|hold| hold.until)), (Mode::Play, Some(NOW + 600)));
    assert_eq!(closed.get(), 0);
    assert_eq!(decide("please rebase the lane", &table, &limits, dir.path(), NOW + 5, || unreachable!()), None);
    assert_eq!(mode::read(dir.path()).mode, Mode::Play);
    let off = decide("done testing", &table, &limits, dir.path(), NOW + 60, teardown).unwrap();
    assert_eq!(off.lines().count(), 1);
    assert!(off.starts_with("capacity mode: quiet off from \"done testing\": closed Warcraft III"), "{off}");
    let released = mode::read(dir.path());
    assert_eq!((released.mode, released.hold), (Mode::Desktop, None));
    assert_eq!(closed.get(), 1);
}

#[test]
fn a_lease_lasts_as_long_as_the_prompt_says_or_its_test_kind_and_more_extends_it() {
    let (table, limits) = (Table::bundled(), Limits::bundled());
    assert_eq!(lease_minutes("test the wisp map", &table, limits.lease_minutes), 10);
    assert_eq!(lease_minutes("testing wisp online with people", &table, limits.lease_minutes), 40);
    assert_eq!(lease_minutes("playtest makgora multiplayer", &table, limits.lease_minutes), 40);
    assert_eq!(lease_minutes("test the wisp map, a perf capture", &table, limits.lease_minutes), 20);
    assert_eq!(lease_minutes("testing wisp online for 20 minutes", &table, limits.lease_minutes), 20);
    assert_eq!(lease_minutes("test wisp 25min", &table, limits.lease_minutes), 25);
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(decide("more", &table, &limits, dir.path(), NOW, || unreachable!()), None, "with no lease on, more does nothing");
    decide("testing wisp online", &table, &limits, dir.path(), NOW, || unreachable!()).unwrap();
    assert_eq!(mode::read(dir.path()).hold.map(|hold| (hold.until, hold.minutes)), Some((NOW + 2400, 40)));
    let more = decide("more", &table, &limits, dir.path(), NOW + 2300, || unreachable!()).unwrap();
    assert!(more.contains("\"more\" adds 40 min"), "{more}");
    assert_eq!(mode::read(dir.path()).hold.map(|hold| hold.until), Some(NOW + 4800));
    decide("test the wisp map", &table, &limits, dir.path(), NOW + 2400, || unreachable!()).unwrap();
    assert_eq!(mode::read(dir.path()).hold.map(|hold| (hold.until, hold.minutes)), Some((NOW + 5400, 10)), "a new testing phrase extends by its own kind");
    assert_eq!(classify("extend it", &table), Some(Phrase::Extend));
    assert_eq!(classify("tell me more about the lander", &table), None);
}
