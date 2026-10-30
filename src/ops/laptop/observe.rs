use std::fs;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use toml_edit::DocumentMut;

use super::ctx::Ctx;
use super::die::Die;
use super::stamp::{iso, parse, write_atomic};
use super::state::{GRACE_CLOSE, GRACE_DRAIN, State};
use super::zone::Window;

const WRONG: &str = "observe wants gaming/sleeping/battery_low booleans and idle boolean or null";
const CLEAR_SECONDS: i64 = 600;
const ACTIVE_SECONDS: i64 = 300;
const OBSERVATION_FRESH_SECONDS: i64 = 120;
const AUTO_REASONS: [&str; 4] = ["gaming", "sleep", "battery", "drained (battery)"];
const AUTO_DRAINED: [&str; 3] = ["drained (active)", "drained (battery)", "drained (window)"];

static IDLE_REOPEN_SECONDS: LazyLock<i64> = LazyLock::new(|| {
    include_str!("../../../harness/defaults.toml")
        .parse::<DocumentMut>()
        .ok()
        .and_then(|document| document["laptop_idle_reopen_sec"]["value"].as_integer())
        .expect("harness/defaults.toml holds laptop_idle_reopen_sec")
});

#[derive(Default, Deserialize, Serialize)]
struct Observations {
    at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    idle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clear: Option<String>,
}

struct Sample {
    gaming: bool,
    sleeping: bool,
    battery_low: bool,
    idle: Option<bool>,
    battery_known: bool,
}

struct Frame {
    state: State,
    at: i64,
    auto: bool,
    edge: Option<(i64, &'static str)>,
    in_window: bool,
}

fn sample(arguments: &[String]) -> Option<Sample> {
    let [text] = arguments else {
        return None;
    };
    let value: Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;
    let flag = |key: &str| object.get(key).and_then(Value::as_bool);
    let idle = match object.get("idle") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(flag)) => Some(*flag),
        Some(_) => return None,
    };
    Some(Sample {
        gaming: flag("gaming")?,
        sleeping: flag("sleeping")?,
        battery_low: flag("battery_low")?,
        idle,
        battery_known: !matches!(object.get("battery"), None | Some(Value::Null)),
    })
}

fn hold_for(state: &State, sample: &Sample) -> Option<&'static str> {
    if sample.gaming {
        Some("gaming")
    } else if sample.sleeping && (state.state == "available" || state.reason == "sleep") {
        Some("sleep")
    } else if sample.battery_low {
        Some("battery")
    } else {
        None
    }
}

impl Ctx {
    pub(super) fn observe(&self, arguments: &[String]) -> Result<String, Die> {
        let sample = sample(arguments).ok_or_else(|| Die::new(2, WRONG))?;
        let at = self.now()?;
        let stem = self.state.file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let path = self.sibling(&format!("{stem}-observations.json"));
        let previous = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Observations>(&text).ok())
            .filter(|previous| parse(&previous.at).is_ok_and(|then| (0..=OBSERVATION_FRESH_SECONDS).contains(&(at - then))))
            .unwrap_or_default();
        let now = iso(at);
        let carry = |active: bool, before: &Option<String>| active.then(|| before.clone().unwrap_or_else(|| now.clone()));
        let mut seen = Observations {
            at: now.clone(),
            active: carry(sample.idle == Some(false), &previous.active),
            idle: carry(sample.idle == Some(true), &previous.idle),
            clear: None,
        };
        if !sample.gaming {
            seen.clear = Some(previous.clear.clone().unwrap_or(now));
        }
        let state = match self.load() {
            Some(state) => Some(self.react(state, &sample, &seen, at)?),
            None => None,
        };
        let text = serde_json::to_string(&seen).map_err(|error| Die::new(1, error.to_string()))?;
        write_atomic(&path, &format!("{text}\n"))?;
        Ok(state.map_or_else(String::new, |state| format!("laptop: {}\n", self.describe(&state))))
    }

    fn react(&self, state: State, sample: &Sample, seen: &Observations, at: i64) -> Result<State, Die> {
        let auto = matches!(state.by.as_str(), "laptop-auto" | "window") || (state.by == "laptop tick" && AUTO_DRAINED.contains(&state.reason.as_str()));
        if state.state != "available" && !auto {
            return Ok(state);
        }
        let edge = Window::read(&self.window).last_edge(at)?;
        let frame = Frame { state, at, auto, edge, in_window: edge.is_some_and(|(_, target)| target == "available") };
        if let Some(hold) = hold_for(&frame.state, sample) {
            return self.held(&frame, hold);
        }
        if auto && AUTO_REASONS.contains(&frame.state.reason.as_str()) {
            return self.cleared(&frame, sample, seen);
        }
        if let Some(settled) = self.active(&frame, seen)? {
            return Ok(settled);
        }
        if let Some(idle) = seen.idle.as_ref().filter(|_| auto && !sample.battery_known && frame.state.state != "available")
            && at - parse(idle)? >= *IDLE_REOPEN_SECONDS
        {
            return self.write("available", at, "idle", "laptop-auto", None, None);
        }
        Ok(frame.state)
    }

    fn active(&self, frame: &Frame, seen: &Observations) -> Result<Option<State>, Die> {
        let Some(active) = seen.active.as_ref().filter(|_| frame.in_window || frame.auto) else {
            return Ok(None);
        };
        if frame.state.until.as_deref().is_some_and(|until| until > iso(frame.at).as_str()) {
            return Ok(None);
        }
        let floor = frame.edge.map_or(frame.at - 86_400, |(edge, _)| edge);
        if frame.at - parse(active)?.max(floor) < ACTIVE_SECONDS {
            return Ok(None);
        }
        if frame.state.state == "available" {
            return self.write("draining", frame.at, "active", "laptop-auto", None, Some(GRACE_DRAIN)).map(Some);
        }
        Ok(Some(frame.state.clone()))
    }

    fn held(&self, frame: &Frame, hold: &str) -> Result<State, Die> {
        if frame.auto && (frame.state.reason == hold || frame.state.reason == format!("drained ({hold})")) {
            return Ok(frame.state.clone());
        }
        let expiry = frame.state.until.as_deref().map(parse).transpose()?;
        let (target, grace) = if hold == "battery" { ("draining", GRACE_DRAIN) } else { ("unavailable", GRACE_CLOSE) };
        self.write(target, frame.at, hold, "laptop-auto", expiry, Some(grace))
    }

    fn cleared(&self, frame: &Frame, sample: &Sample, seen: &Observations) -> Result<State, Die> {
        let reason = frame.state.reason.as_str();
        let clear = match (&seen.clear, reason) {
            (Some(clear), "gaming") => frame.at - parse(clear)? >= CLEAR_SECONDS,
            (None, "gaming") => false,
            _ => reason == "sleep" || !sample.battery_known,
        };
        if !clear {
            return Ok(frame.state.clone());
        }
        let ahead = iso(frame.at);
        let still_held = frame.state.until.as_deref().filter(|until| *until > ahead.as_str()).map(parse).transpose()?;
        let open = frame.in_window || still_held.is_some();
        self.write(if open { "available" } else { "unavailable" }, frame.at, "window", "laptop-auto", still_held, None)
    }
}
