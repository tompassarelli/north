/// A session the reaper retired, whose final report is the only account of its run when no provider run path recorded it.
#[derive(Clone, Debug, PartialEq)]
pub struct RetiredRun {
    pub name: String,
    pub conversation: Option<String>,
    pub account: String,
    pub provider: String,
    pub model: Option<String>,
    pub item: Option<String>,
    pub box_name: Option<String>,
    pub started_ms: u64,
    pub ended_ms: u64,
    pub outcome: &'static str,
    pub report: String,
}

/// Records a finished conversation's run in the worker ledger as its host exits, so a run is not left to the backstop timer.
pub trait RunRecording: Send + Sync {
    fn ended(&self, conversation: &str);

    fn retired(&self, _run: &RetiredRun) {}
}

pub struct NoRecording;

impl RunRecording for NoRecording {
    fn ended(&self, _conversation: &str) {}
}
