use std::path::PathBuf;

use north_base::foundation::offload::URGENT_ENV;
use north_base::platform::session_scope::{SESSION_UNIT_PREFIX, WORKER_BROKER_UNIT};

const ANCESTORS: usize = 64;

pub struct Places {
    pub fleet: PathBuf,
    pub home: PathBuf,
    pub procs: PathBuf,
    pub cgroups: PathBuf,
}

enum Holder {
    Session(String),
    Held(bool),
    Broker,
}

pub fn caller_urgent() -> bool {
    Places::here().is_some_and(|places| places.process("self"))
}

impl Places {
    pub fn here() -> Option<Self> {
        let uid = rustix::process::getuid().as_raw();
        let home = account_home(&std::fs::read_to_string("/etc/passwd").ok()?, uid)?;
        Some(Self::beside(home.join(".local/state/north/fleet"), home, PathBuf::from("/sys/fs/cgroup")))
    }

    pub fn beside(fleet: PathBuf, home: PathBuf, cgroups: PathBuf) -> Self {
        Self { fleet, home, procs: PathBuf::from("/proc"), cgroups }
    }

    pub fn cgroup(&self, cgroup: &str) -> bool {
        match self.holder(cgroup) {
            Some(Holder::Session(unit)) => crate::fleet::records::launched_urgent_in(&self.fleet, &unit),
            Some(Holder::Held(urgent)) => urgent,
            Some(Holder::Broker) => false,
            None => self.member(cgroup).is_some_and(|pid| self.process(&pid)),
        }
    }

    pub fn process(&self, pid: &str) -> bool {
        let mut below: Option<String> = None;
        let mut current = pid.to_owned();
        for _ in 0..ANCESTORS {
            let Some(cgroup) = self.read(&current, "cgroup") else { return false };
            let Some(parent) = self.parent(&current) else { return false };
            match self.holder(&cgroup) {
                Some(Holder::Session(unit)) => return crate::fleet::records::launched_urgent_in(&self.fleet, &unit),
                Some(Holder::Held(urgent)) => return urgent,
                Some(Holder::Broker) if !self.read(&parent, "cgroup").is_some_and(|above| matches!(self.holder(&above), Some(Holder::Broker))) => {
                    return below.is_some_and(|started| self.mark(&started).as_deref() == Some("1"));
                }
                _ => {}
            }
            if parent == "0" {
                return false;
            }
            below = Some(current);
            current = parent;
        }
        false
    }

    fn holder(&self, cgroup: &str) -> Option<Holder> {
        let path = cgroup.lines().find_map(|line| line.strip_prefix("0::"))?;
        path.split('/').map(str::trim_end).find_map(|segment| {
            if segment.starts_with(SESSION_UNIT_PREFIX) && segment.ends_with(".scope") {
                return Some(Holder::Session(segment.to_owned()));
            }
            if let Some(urgent) = segment.ends_with(".service").then(|| crate::fleet::records::held_urgent(&self.fleet, &self.home, segment)).flatten() {
                return Some(Holder::Held(urgent));
            }
            broker_unit(segment).then_some(Holder::Broker)
        })
    }

    fn member(&self, cgroup: &str) -> Option<String> {
        let path = cgroup.lines().find_map(|line| line.strip_prefix("0::"))?.trim();
        let procs = std::fs::read_to_string(self.cgroups.join(path.trim_start_matches('/')).join("cgroup.procs")).ok()?;
        procs.lines().map(str::trim).find(|pid| !pid.is_empty()).map(str::to_owned)
    }

    fn read(&self, pid: &str, file: &str) -> Option<String> {
        std::fs::read_to_string(self.procs.join(pid).join(file)).ok()
    }

    fn parent(&self, pid: &str) -> Option<String> {
        let stat = self.read(pid, "stat")?;
        stat.rsplit_once(')')?.1.split_whitespace().nth(1).map(str::to_owned)
    }

    fn mark(&self, pid: &str) -> Option<String> {
        let environ = std::fs::read(self.procs.join(pid).join("environ")).ok()?;
        environ.split(|byte| *byte == 0).find_map(|entry| entry.strip_prefix(format!("{URGENT_ENV}=").as_bytes()).map(|value| String::from_utf8_lossy(value).into_owned()))
    }
}

fn broker_unit(segment: &str) -> bool {
    segment.strip_prefix(WORKER_BROKER_UNIT).and_then(|rest| rest.strip_suffix(".service")).is_some_and(|rest| rest.is_empty() || rest.strip_prefix('-').is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit())))
}

fn account_home(passwd: &str, uid: u32) -> Option<PathBuf> {
    passwd.lines().map(|line| line.split(':').collect::<Vec<_>>()).find(|fields| fields.len() >= 7 && fields[2] == uid.to_string() && !fields[5].is_empty()).map(|fields| PathBuf::from(fields[5]))
}
