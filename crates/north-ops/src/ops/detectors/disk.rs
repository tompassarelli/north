use super::{Detector, Finding, Limits};

pub struct Host {
    pub name: String,
    pub free_gib: f64,
}

#[derive(Default)]
pub struct Facts {
    pub owner: Option<String>,
    pub hosts: Vec<Host>,
}

pub fn low(facts: &Facts, now: f64, limits: &Limits) -> Vec<Finding> {
    let day = &crate::foundation::clock::Timestamp::from_unix_seconds_f64(now).iso()[..10];
    let mut findings: Vec<Finding> = facts
        .hosts
        .iter()
        .filter(|host| host.free_gib < limits.disk_floor_gib)
        .map(|host| Finding {
            detector: Detector::LowDisk,
            key: format!("disk:{}:{day}", host.name),
            owner: facts.owner.clone(),
            title: format!("Free disk on {}", host.name),
            detail: format!("The root disk of {} has {:.0} GiB free, under the {:.0} GiB floor where admission defers builds.", host.name, host.free_gib, limits.disk_floor_gib),
            done_when: format!("`df -BG --output=avail /` on {} prints at least {:.0}G and `north flow detect --dry-run` prints no `disk:{}` line.", host.name, limits.disk_floor_gib, host.name),
            instances: vec![host.name.clone()],
        })
        .collect();
    findings.sort_by(|a, b| a.key.cmp(&b.key));
    findings
}
