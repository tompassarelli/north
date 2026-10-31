use super::ctx::Ctx;
use super::die::Die;
use super::stamp::parse;
use super::state::{GRACE_DRAIN, State};
use super::zone::Window;

const AUTO_HOLDS: [&str; 4] = ["gaming", "sleep", "battery", "drained (battery)"];

impl Ctx {
    pub(super) fn tick(&self) -> Result<String, Die> {
        let at = self.now()?;
        let mut current = self.load();
        let edge = Window::read(&self.window).last_edge(at)?;
        let expired = match current.as_ref().and_then(|state| state.until.as_deref()) {
            Some(until) => parse(until)? <= at,
            None => false,
        };
        let stale = match &current {
            None => true,
            Some(state) => state.until.is_none() && edge.is_some_and(|(edge, _)| parse(&state.since).is_ok_and(|since| since < edge)),
        };
        let held = current.as_ref().is_some_and(|state| matches!(state.by.as_str(), "laptop-auto" | "laptop tick") && AUTO_HOLDS.contains(&state.reason.as_str()));
        if (expired || stale) && !held {
            current = Some(self.apply_window(current, edge.map_or("unavailable", |(_, target)| target), at)?);
        }
        let Some(mut state) = current else {
            return Err(Die::new(1, "no state to settle"));
        };
        if state.state == "draining" && self.jobs().map(|jobs| jobs.running) == Some(0) {
            let hold = match &state.until {
                Some(until) => Some(parse(until)?),
                None => None,
            };
            state = self.write("unavailable", at, &format!("drained ({})", state.reason), "laptop tick", hold, None)?;
        }
        Ok(format!("laptop: {}\n", self.describe(&state)))
    }

    fn apply_window(&self, current: Option<State>, target: &str, at: i64) -> Result<State, Die> {
        let drain = || match &self.grace {
            Some(text) => text.parse::<i64>().map_err(|_| Die::new(2, format!("LAPTOP_GRACE wants seconds, got {text}"))),
            None => Ok(GRACE_DRAIN),
        };
        match current {
            Some(state) if target == "available" && state.state == "available" && state.until.is_none() => Ok(state),
            _ if target == "available" => self.write("available", at, "window", "window", None, None),
            Some(state) if state.state == "available" => self.write("draining", at, "window", "window", None, Some(drain()?)),
            Some(state) if state.until.is_none() => Ok(state),
            Some(state) => self.write(&state.state, at, "window", "window", None, state.grace),
            None => self.write("unavailable", at, "window", "window", None, None),
        }
    }
}
