use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use super::{Event, emit, event_line};
use crate::shell::recorded::{self, State};
use crate::shell::settings::Settings;

const BASH: &str = r##"{"time":1762430000.123456,"repo":"north","lane":"north-621-p8","sha":"2971147b7c1f4f3f6b8a2f0a3a8b5f2c9d1e0a77","stage":"dependency gate and scans","status":"wait","detail":"","seconds":0,"pid":4242,"session":"n621-p8","host":"laptop"}
{"time":1762430002.500000,"repo":"wisp","lane":"feature/x","sha":"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef","stage":"exit","status":"refused","detail":"blocking \"scan\" failed\\ at C:\\path and more","seconds":0,"pid":77,"session":"","host":""}
{"time":1762430003.000005,"repo":"smashcraft","lane":"lane-1","sha":"","stage":"land checks","status":"end","detail":"","seconds":12.3,"pid":1,"session":"s-2","host":"box.local"}
{"time":1762430004.999999,"repo":"b.git","lane":"odd\"lane\\","sha":"0123456789012345678901234567890123456789","stage":"unicode ✓ stage","status":"failed","detail":"clean — ✓","seconds":0.0,"pid":99999,"session":"sess \"q\"","host":"h\\1"}
"##;

fn event<'a>(
    micros: u64,
    origin_url: &'a str,
    lane: &'a str,
    sha: &'a str,
    pid: u32,
    session: &'a str,
    host: &'a str,
    stage: &'a str,
    status: &'a str,
    detail: &'a str,
    tenths: Option<u64>,
) -> Event<'a> {
    Event {
        micros,
        origin_url,
        lane,
        sha,
        stage,
        status,
        detail,
        tenths,
        pid,
        session,
        host,
    }
}

fn goldens() -> [Event<'static>; 4] {
    [
        event(
            1_762_430_000_123_456,
            "git@github.com:tompassarelli/north.git",
            "north-621-p8",
            "2971147b7c1f4f3f6b8a2f0a3a8b5f2c9d1e0a77",
            4242,
            "n621-p8",
            "laptop",
            "dependency gate and scans",
            "wait",
            "",
            None,
        ),
        event(
            1_762_430_002_500_000,
            "https://github.com/tompassarelli/wisp.git",
            "feature/x",
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            77,
            "",
            "",
            "exit",
            "refused",
            "blocking \"scan\" failed\\ at C:\\path\tand\u{1}more",
            None,
        ),
        event(
            1_762_430_003_000_005,
            "/srv/git/smashcraft",
            "lane-1",
            "",
            1,
            "s-2",
            "box.local",
            "land checks",
            "end",
            "",
            Some(123),
        ),
        event(
            1_762_430_004_999_999,
            "ssh://host/a/b.git.git",
            "odd\"lane\\",
            "0123456789012345678901234567890123456789",
            99999,
            "sess \"q\"",
            "h\\1",
            "unicode ✓ stage",
            "failed",
            "clean — ✓",
            Some(0),
        ),
    ]
}

#[test]
fn event_line_equals_the_lines_bash_printed_for_the_same_fields() {
    let rendered: String = goldens().iter().map(event_line).collect();
    assert_eq!(rendered, BASH);
}

#[test]
fn emit_appends_the_bash_lines_and_nudges_the_watcher_only_for_refused_and_failed() {
    let state = Rc::new(RefCell::new(State::default()));
    let mut settings = Settings::from_env();
    settings.watcher = Some(PathBuf::from("/run/north"));
    let shell = recorded::shell(&state, settings);
    for golden in goldens() {
        emit(&shell, &golden);
    }
    assert_eq!(state.borrow().events.concat(), BASH);
    assert_eq!(
        state.borrow().detached,
        vec![
            (
                PathBuf::from("/run/north"),
                vec!["watch".to_owned(), "--once".to_owned()]
            );
            2
        ]
    );
    let quiet = Rc::new(RefCell::new(State::default()));
    let mut settings = Settings::from_env();
    settings.watcher = None;
    emit(
        &recorded::shell(&quiet, settings),
        &event(
            1, "o/r.git", "lane", "", 1, "", "", "exit", "refused", "", None,
        ),
    );
    assert!(quiet.borrow().detached.is_empty());
}
