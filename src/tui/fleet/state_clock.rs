use std::collections::BTreeMap;

use crate::fleet::model::Status;

#[derive(Default)]
pub struct StateClock {
    entered: BTreeMap<String, (Status, u64)>,
}

impl StateClock {
    pub fn observe(&mut self, name: &str, status: Status, first_seen_ms: u64, now_ms: u64) {
        let entry = self.entered.entry(name.to_owned()).or_insert((status, first_seen_ms));
        if entry.0 != status {
            *entry = (status, now_ms);
        }
    }

    pub fn since(&self, name: &str) -> Option<u64> {
        self.entered.get(name).map(|(_, since)| *since)
    }

    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.entered.retain(|name, _| keep(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_keeps_its_state_start_until_its_status_changes() {
        let mut clock = StateClock::default();
        clock.observe("a", Status::Idle, 100, 1_000);
        clock.observe("a", Status::Idle, 100, 2_000);
        assert_eq!(clock.since("a"), Some(100));
        clock.observe("a", Status::Working, 100, 3_000);
        clock.observe("a", Status::Working, 100, 4_000);
        assert_eq!(clock.since("a"), Some(3_000));
        clock.retain(|name| name != "a");
        assert_eq!(clock.since("a"), None);
    }
}
