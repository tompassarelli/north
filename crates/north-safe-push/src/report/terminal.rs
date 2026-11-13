use north_base::build_inputs::{LIVE_FIX, promotion_slot, promotion_slot_sec, runtime_input};
use north_base::foundation::step::cancel_origin;

#[derive(Debug, Eq, PartialEq)]
pub enum Terminal {
    Published(String),
    Queued(String),
    Clean(String),
    Refused(String),
    Cancelled,
}

pub fn receipt(terminal: &Terminal) -> (String, u8) {
    match terminal {
        Terminal::Published(line) | Terminal::Queued(line) | Terminal::Clean(line) => {
            (format!("safe-push: {line}"), 0)
        }
        Terminal::Refused(reason) => (format!("safe-push: refused {reason}"), 1),
        Terminal::Cancelled => (cancelled_line(), 130),
    }
}

/// The one place a cancel receipt is written: the bare line before any signal named its sender, else the signal with
/// the pid, uid and command name of the process that sent it.
pub fn cancelled_line() -> String {
    match cancel_origin() {
        Some(origin) => format!("safe-push: cancelled ({origin})"),
        None => "safe-push: cancelled".to_owned(),
    }
}

pub fn promotion_note<'a>(changed: impl IntoIterator<Item = &'a str>, now: i64) -> Option<String> {
    changed.into_iter().any(runtime_input).then(|| {
        let slot = promotion_slot(now, promotion_slot_sec()) + promotion_slot_sec();
        format!("safe-push: this landing changes a runtime input, so it goes live at the {:02}:{:02} UTC promotion slot; a `{LIVE_FIX}: REASON` trailer on the commit promotes it at once", slot.rem_euclid(86_400) / 3600, slot.rem_euclid(3600) / 60)
    })
}

#[cfg(test)]
mod tests {
    use super::{Terminal, promotion_note, receipt};

    #[test]
    fn a_north_landing_that_waits_for_a_slot_names_the_slot_and_the_live_fix_trailer() {
        let at_0130 = 1_762_430_000 - 1_762_430_000 % 86_400 + 5400;
        let note = promotion_note(["docs/a.md", "crates/x/src/lib.rs"], at_0130).unwrap();
        assert!(note.contains("04:00 UTC promotion slot") && note.contains("Live-fix: REASON"), "{note}");
        assert_eq!(promotion_note(["docs/a.md"], at_0130), None);

        assert_eq!(
            receipt(&Terminal::Refused("not on a branch".to_owned())),
            ("safe-push: refused not on a branch".to_owned(), 1)
        );
        assert_eq!(
            receipt(&Terminal::Cancelled),
            ("safe-push: cancelled".to_owned(), 130)
        );
        assert_eq!(
            receipt(&Terminal::Clean("✓ clean".to_owned())),
            ("safe-push: ✓ clean".to_owned(), 0)
        );
        assert_eq!(
            receipt(&Terminal::Queued("queued".to_owned())),
            ("safe-push: queued".to_owned(), 0)
        );
        assert_eq!(
            receipt(&Terminal::Published("pushed".to_owned())),
            ("safe-push: pushed".to_owned(), 0)
        );
    }
}
