use std::cell::Cell;

use super::quiet_phrase::{Phrase, Table, classify, decide};
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
    assert!(on.starts_with("capacity mode: quiet on from \"test wisp\""), "{on}");
    let held = mode::read(dir.path());
    assert_eq!((held.mode, held.hold.map(|hold| hold.until)), (Mode::Play, Some(NOW + 7200)));
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
