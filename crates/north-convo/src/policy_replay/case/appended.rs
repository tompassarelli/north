use std::fs;
use std::path::PathBuf;

use regex::Regex;

use super::source::Case;
use crate::fleet::model::Role;
use crate::fleet::prompt::{PromptInputs, PromptMode, StandingOrders, render_from};
use crate::fleet::records::{conversation_record, named_record_before};
use crate::foundation::clock::Timestamp;
use crate::fleet::runtime::DEFAULT_FOREGROUND_CAP_SECS;

pub struct Launch {
    role: Role,
    mode: PromptMode,
    foreground_cap_secs: u64,
    orders: String,
    brief: Option<String>,
}

fn north_state() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state/north")
}

fn orders_by(orders: &str, day: &str) -> String {
    let dated = Regex::new(r"\((\d{4}-\d{2}-\d{2})\)").expect("a valid date pattern");
    orders.lines().filter(|line| dated.captures(line).is_none_or(|found| &found[1] <= day)).map(|line| format!("{line}\n")).collect()
}

pub fn launch(case: &Case, fleet_session: Option<&str>) -> Option<Launch> {
    let fleet = north_state().join("fleet");
    let record = match fleet_session {
        Some(name) => {
            let cut_ms = Timestamp::parse_loose(&case.cut_at).map_or(u64::MAX, |stamp| (stamp.unix_seconds_f64() * 1000.0) as u64);
            let found = named_record_before(&fleet, name, cut_ms);
            if found.is_none() {
                eprintln!("policy-replay case: {}: no record of fleet session {name} created before {}, so its arms append no fleet prompt", case.id, case.cut_at);
            }
            found?
        }
        None => conversation_record(&fleet, &case.session)?,
    };
    let orders = StandingOrders::in_state(&north_state()).read().unwrap_or_default();
    Some(Launch {
        role: record.role(),
        mode: record.prompt_mode,
        foreground_cap_secs: record.foreground_cap_secs.unwrap_or(DEFAULT_FOREGROUND_CAP_SECS),
        orders: orders_by(&orders, case.cut_at.get(..10).unwrap_or("9999-12-31")),
        brief: record.system_prompt.as_deref().and_then(|path| {
            let brief = fs::read_to_string(path).ok();
            if brief.is_none() {
                eprintln!("policy-replay case: {}: the brief {} that session {} was launched with is gone, so its arms append the fleet prompt alone", case.id, path.display(), record.name);
            }
            brief
        }),
    })
}

impl Launch {
    pub fn appended(&self, template: &str) -> String {
        let inputs = PromptInputs { role: self.role, mode: self.mode, orders: &self.orders, foreground_cap_secs: self.foreground_cap_secs, extra: self.brief.as_deref() };
        render_from(template, &inputs)
    }
}
