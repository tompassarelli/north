use super::event::{Event, emit, repository};
use crate::shell::Shell;

pub struct Stage<'a> {
    pub name: &'a str,
    pub origin_url: &'a str,
    pub branch: &'a str,
    pub sha: &'a str,
    pub started_ms: u64,
    pub previous_ms: u64,
    pub now_ms: u64,
}

impl Stage<'_> {
    fn stage_ms(&self) -> u64 {
        self.now_ms.saturating_sub(self.previous_ms)
    }

    fn total_ms(&self) -> u64 {
        self.now_ms.saturating_sub(self.started_ms)
    }
}

pub fn stage_line(stage: &Stage) -> String {
    let (spent, total) = (stage.stage_ms(), stage.total_ms());
    format!(
        "safe-push: stage {}: {}.{:03} s (+{}.{:03} s since start)",
        stage.name,
        spent / 1000,
        spent % 1000,
        total / 1000,
        total % 1000
    )
}

pub fn stage_row(stage: &Stage) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\n",
        stage.started_ms / 1000,
        repository(stage.origin_url),
        stage.branch,
        stage.name,
        stage.stage_ms(),
        stage.total_ms()
    )
}

pub fn record(shell: &Shell, stage: &Stage) {
    println!("{}", stage_line(stage));
    let _ = shell.files.append_stage(&stage_row(stage));
    let event = Event {
        micros: now_micros(),
        origin_url: stage.origin_url,
        lane: stage.branch,
        sha: stage.sha,
        stage: stage.name,
        status: "end",
        detail: "",
        tenths: Some(stage.stage_ms() / 100),
        pid: shell.settings.pid,
        session: &shell.settings.fleet_session,
        host: &shell.settings.host,
    };
    emit(shell, &event);
}

fn now_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_micros()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
#[path = "stage_tests.rs"]
mod tests;
