use serde::Serialize;

use super::ctx::Ctx;
use super::die::Die;
use super::stamp::iso;
use super::state::{GRACE_CLOSE, GRACE_DRAIN, State, deadline};
use super::zone::Window;

#[derive(Serialize)]
struct StatusJson<'a> {
    #[serde(flatten)]
    state: &'a State,
    #[serde(skip_serializing_if = "Option::is_none")]
    deadline: Option<i64>,
}

impl Ctx {
    pub(super) fn word(&self, verb: &str, args: &[String]) -> Result<String, Die> {
        let at = self.now()?;
        let (hold, args) = match args.first().map(String::as_str) {
            Some("until") => {
                let hhmm = args.get(1).ok_or_else(|| Die::new(2, "until wants HH:MM"))?;
                (Some(Window::read(&self.window).until_at(hhmm, at)?), &args[2..])
            }
            _ => (None, args),
        };
        let current = self.load();
        let verb = match verb {
            "toggle" if current.as_ref().is_some_and(|state| state.state == "available") => "close",
            "toggle" => "open",
            other => other,
        };
        let reason = if args.is_empty() { verb.to_owned() } else { args.join(" ") };
        let by = self.actor();
        let written = if verb == "open" {
            self.write("available", at, &reason, &by, hold, None)?
        } else {
            let grace = match &self.grace {
                Some(text) => text.parse::<i64>().map_err(|_| Die::new(2, format!("LAPTOP_GRACE wants seconds, got {text}")))?,
                None if verb == "close" => GRACE_CLOSE,
                None => GRACE_DRAIN,
            };
            if current.is_some_and(|state| state.state == "unavailable") {
                self.write("unavailable", at, &reason, &by, hold, None)?
            } else {
                self.write("draining", at, &reason, &by, hold, Some(grace))?
            }
        };
        Ok(format!("laptop: {}\n", self.describe(&written)))
    }

    pub(super) fn status(&self, args: &[String]) -> Result<String, Die> {
        let current = self.load();
        if args == ["--json"] {
            let state = match current {
                Some(state) => state,
                None => State { state: "unavailable".into(), since: iso(self.now()?), reason: "no state".into(), by: "-".into(), until: None, grace: None },
            };
            let line = serde_json::to_string(&StatusJson { deadline: deadline(&state), state: &state }).map_err(|error| Die::new(1, error.to_string()))?;
            return Ok(format!("{line}\n"));
        }
        if !args.is_empty() {
            return Err(Die::new(2, "status takes only --json"));
        }
        let jobs = self.jobs();
        let waiting = jobs.as_ref().and_then(|jobs| jobs.waiting.as_deref()).map_or_else(String::new, |reason| format!("; {reason}"));
        let Some(state) = current else {
            return Ok(format!("laptop: no state yet (unavailable){waiting}\n"));
        };
        let counts = jobs.map_or_else(|| "ledger unreachable".to_owned(), |jobs| format!("{} running, {} queued", jobs.running, jobs.queued));
        Ok(format!("laptop: {} ({counts}{waiting})\n", self.describe(&state)))
    }
}
