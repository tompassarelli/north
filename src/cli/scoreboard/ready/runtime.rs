//! The bars read from the runtime's own records, one fleet for every repository: the pinned runtime's journal, how long
//! a landing took to reach every host, and how far a host's processes lag the live runtime.
use super::{Bar, DAY_S, Reading, Records, Window, percentile, propagation};

impl Records {
    pub(super) fn runtime(&self, bar: Bar, window: Window) -> Option<Reading> {
        let start = if matches!(bar, Bar::RuntimeLive) { window.end - DAY_S } else { window.start };
        let pins = north_runtime::runtime::pin::bars(&self.pins, start, window.end);
        let p90 = |seconds: &[f64]| (!seconds.is_empty()).then(|| (percentile(seconds, 0.9), seconds.len()));
        let reading = match bar {
            Bar::RuntimeBreaks => pins.breaks(),
            Bar::RuntimeRollback => pins.rollback(),
            Bar::RuntimeAge => pins.age(),
            Bar::RuntimeCanary => p90(&pins.canary_seconds),
            Bar::RuntimeShip => p90(&pins.ship_seconds),
            _ => p90(&pins.due_to_live_seconds),
        };
        reading.map(Reading::from)
    }

    pub(super) fn propagation_held(&self, bar: Bar, window: Window) -> Option<Reading> {
        match bar {
            Bar::RuntimeLag => north_runtime::runtime::lag::samples::worst_in(&self.lags, window.start, window.end).map(Reading::from),
            _ => propagation::firn_closure_p90(&self.propagation, window.start, window.end).map(Reading::from),
        }
    }
}
