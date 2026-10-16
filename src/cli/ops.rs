use std::process::ExitCode;

use crate::fleet::model::{Delivery, Status};
use crate::ops::facts::SessionFact;
use crate::ops::{self, Wiring};

const PROXY: &str = "proxy";

pub(super) fn run(arguments: &[String]) -> ExitCode {
    let wiring = Wiring { sessions: &sessions, proxy: &tell_proxy };
    ExitCode::from(ops::run(arguments, &wiring))
}

fn sessions() -> Vec<SessionFact> {
    let Ok(fleet) = super::fleet::open_fleet() else { return Vec::new() };
    let Ok(records) = fleet.sessions() else { return Vec::new() };
    records
        .iter()
        .map(|record| {
            let status = fleet.status(record);
            SessionFact {
                name: record.name.clone(),
                account: format!("{}:{}", record.provider.label(), record.account),
                live: status != Status::Exited,
                working: status == Status::Working,
                created_at: (record.created_ms / 1000) as i64,
                host_dir: record.host_dir.display().to_string(),
            }
        })
        .collect()
}

fn tell_proxy(text: &str) -> Result<(), String> {
    let fleet = super::fleet::open_fleet().map_err(|error| error.to_string())?;
    fleet.send(PROXY, text, Delivery::Now).map_err(|error| error.to_string())
}
