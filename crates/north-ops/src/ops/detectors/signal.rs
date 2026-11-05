use super::{Detector, Finding, Limits};

pub struct Signal {
    pub name: String,
    pub writer: String,
    pub cadence_s: f64,
    pub owner: Option<String>,
    pub last_written: Option<f64>,
}

#[derive(Default)]
pub struct Facts {
    pub signals: Vec<Signal>,
}

fn clock(at: f64) -> String {
    crate::foundation::clock::Timestamp::from_unix_seconds_f64(at).iso()
}

pub fn silent(facts: &Facts, now: f64, limits: &Limits) -> Vec<Finding> {
    let mut findings: Vec<Finding> = facts
        .signals
        .iter()
        .filter_map(|signal| {
            let last = signal.last_written?;
            let limit = limits.silence_cadences * signal.cadence_s;
            if now - last <= limit {
                return None;
            }
            Some(Finding {
                detector: Detector::SilentSignal,
                key: format!("signal:{}:{}", signal.name, last as i64),
                owner: signal.owner.clone(),
                title: format!("Restore the silent signal {}", signal.name),
                detail: format!("{} last wrote {} ({:.0} min ago) against a declared cadence of {:.0} min; its writer is {}.", signal.name, clock(last), (now - last) / 60.0, signal.cadence_s / 60.0, signal.writer),
                done_when: format!("`north flow detect --dry-run` prints no `signal:{}` line after a fresh write of {}, and the writer's next write lands within its declared cadence.", signal.name, signal.name),
                instances: vec![signal.name.clone()],
            })
        })
        .collect();
    findings.sort_by(|a, b| a.key.cmp(&b.key));
    findings
}
