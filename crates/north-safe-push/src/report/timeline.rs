use north_base::foundation::clock::Timestamp;

use super::event::{Event, emit, event_line};
use super::stage::{Stage, record};
use crate::shell::Shell;

pub struct Timeline<'a> {
    shell: &'a Shell,
    started_ms: u64,
    previous_ms: u64,
    origin_url: String,
    branch: String,
    sha: String,
}

impl<'a> Timeline<'a> {
    pub fn start(shell: &'a Shell) -> Self {
        let now = now_ms();
        Self { shell, started_ms: now, previous_ms: now, origin_url: String::new(), branch: String::new(), sha: String::new() }
    }

    pub fn announce(&self) {
        println!("safe-push: start {}", Timestamp::now().iso_millis());
    }

    pub fn identify(&mut self, origin_url: &str, branch: &str, sha: &str) {
        self.origin_url = origin_url.to_owned();
        self.branch = branch.to_owned();
        self.sha = sha.to_owned();
    }

    pub fn stage(&mut self, name: &str) {
        let now_ms = now_ms();
        let stage = Stage {
            name,
            origin_url: &self.origin_url,
            branch: &self.branch,
            sha: &self.sha,
            started_ms: self.started_ms,
            previous_ms: self.previous_ms,
            now_ms,
        };
        record(self.shell, &stage);
        self.previous_ms = now_ms;
    }

    pub fn event(&self, stage: &str, status: &str, detail: &str) {
        emit(self.shell, &self.of(stage, status, detail));
    }

    pub fn wait_line(&self, stage: &str) -> String {
        event_line(&self.of(stage, "wait", ""))
    }

    fn of<'e>(&'e self, stage: &'e str, status: &'e str, detail: &'e str) -> Event<'e> {
        Event {
            micros: now_micros(),
            origin_url: &self.origin_url,
            lane: &self.branch,
            sha: &self.sha,
            stage,
            status,
            detail,
            tenths: None,
            pid: self.shell.settings.pid,
            session: &self.shell.settings.fleet_session,
            host: &self.shell.settings.host,
        }
    }
}

fn now_ms() -> u64 {
    u64::try_from(Timestamp::now().unix_millis()).unwrap_or(0)
}

fn now_micros() -> u64 {
    u64::try_from(Timestamp::now().unix_nanos() / 1000).unwrap_or(0)
}
