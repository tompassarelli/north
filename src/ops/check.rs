use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::expect::{Outcome, State};

pub(crate) trait Courier {
    fn phone(&self, title: &str, text: &str) -> Result<String, String>;
    fn proxy(&self, text: &str) -> Result<(), String>;
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct Alarm {
    pub since: i64,
    pub line: String,
    pub phoned: bool,
    pub told_proxy: bool,
    pub cleared: bool,
}

pub(crate) type Alarms = BTreeMap<String, Alarm>;

#[derive(Debug, Default, Serialize)]
pub(crate) struct Report {
    pub breached: Vec<String>,
    pub raised: Vec<String>,
    pub cleared: Vec<String>,
    pub failures: Vec<String>,
    pub deliveries: Vec<String>,
}

pub(crate) fn sound(outcomes: &[Outcome], alarms: &mut Alarms, courier: &dyn Courier, now: i64) -> Report {
    let mut report = Report::default();
    for outcome in outcomes {
        match outcome.state {
            State::Breached => {
                report.breached.push(outcome.id.clone());
                let alarm = alarms.entry(outcome.id.clone()).or_insert_with(|| Alarm { since: now, ..Alarm::default() });
                alarm.line = outcome.line();
                alarm.cleared = false;
            }
            State::Holds | State::Inactive => {
                if let Some(alarm) = alarms.get_mut(&outcome.id) {
                    alarm.cleared = true;
                }
            }
            State::Unknown => {}
        }
    }
    for id in alarms.keys().filter(|id| !outcomes.iter().any(|outcome| outcome.id == **id)).cloned().collect::<Vec<_>>() {
        alarms.get_mut(&id).map(|alarm| alarm.cleared = true);
    }
    raise(alarms, courier, &mut report);
    clear(alarms, courier, &mut report);
    report
}

fn raise(alarms: &mut Alarms, courier: &dyn Courier, report: &mut Report) {
    let unphoned: Vec<String> = alarms.iter().filter(|(_, alarm)| !alarm.cleared && !alarm.phoned).map(|(id, _)| id.clone()).collect();
    if !unphoned.is_empty() {
        let text = unphoned.iter().map(|id| alarms[id].line.clone()).collect::<Vec<_>>().join("\n");
        match courier.phone(&title("breached", &unphoned), &text) {
            Ok(delivery) => {
                report.deliveries.push(delivery);
                unphoned.iter().for_each(|id| alarms.get_mut(id).into_iter().for_each(|alarm| alarm.phoned = true));
                report.raised.extend(unphoned.iter().cloned());
            }
            Err(error) => report.failures.push(format!("phone: {error}")),
        }
    }
    let untold: Vec<String> = alarms.iter().filter(|(_, alarm)| !alarm.cleared && !alarm.told_proxy).map(|(id, _)| id.clone()).collect();
    for id in untold {
        match courier.proxy(&format!("[ops alert] {}", alarms[&id].line)) {
            Ok(()) => alarms.get_mut(&id).into_iter().for_each(|alarm| alarm.told_proxy = true),
            Err(error) => report.failures.push(format!("proxy: {error}")),
        }
    }
}

fn clear(alarms: &mut Alarms, courier: &dyn Courier, report: &mut Report) {
    let cleared: Vec<String> = alarms.iter().filter(|(_, alarm)| alarm.cleared).map(|(id, _)| id.clone()).collect();
    if cleared.is_empty() {
        return;
    }
    let phoned: Vec<&String> = cleared.iter().filter(|id| alarms[*id].phoned).collect();
    let phone = match phoned.is_empty() {
        true => Ok(()),
        false => courier
            .phone(&title("cleared", &cleared), &phoned.iter().map(|id| format!("{id} cleared")).collect::<Vec<_>>().join("\n"))
            .map(|delivery| report.deliveries.push(delivery)),
    };
    let told: Vec<&String> = cleared.iter().filter(|id| alarms[*id].told_proxy).collect();
    let proxy = match told.is_empty() {
        true => Ok(()),
        false => courier.proxy(&format!("[ops cleared] {}", told.iter().map(|id| id.as_str()).collect::<Vec<_>>().join(", "))),
    };
    match (phone, proxy) {
        (Ok(()), Ok(())) => {
            cleared.iter().for_each(|id| {
                alarms.remove(id);
            });
            report.cleared.extend(cleared);
        }
        (phone, proxy) => report.failures.extend(phone.err().into_iter().chain(proxy.err()).map(|error| format!("cleared: {error}"))),
    }
}

fn title(verb: &str, ids: &[String]) -> String {
    match ids {
        [one] => format!("north: {one} {verb}"),
        many => format!("north: {} expectations {verb}", many.len()),
    }
}
