/// Records a finished conversation's run in the worker ledger as its host exits, so a run is not left to the backstop timer.
pub trait RunRecording: Send + Sync {
    fn ended(&self, conversation: &str);
}

pub struct NoRecording;

impl RunRecording for NoRecording {
    fn ended(&self, _conversation: &str) {}
}
