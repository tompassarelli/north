use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::session_host::FrameWatch;

use super::records::find_session;
use super::runtime::{Runtime, now_ms};
use super::turns::{RunRecord, RunSink, Turn, Turns};

pub struct TurnWatch {
    runtime: Arc<dyn Runtime>,
    sink: Arc<dyn RunSink>,
    root: PathBuf,
    host_dir: PathBuf,
    turns: Turns,
}

impl TurnWatch {
    pub fn new(runtime: Arc<dyn Runtime>, sink: Arc<dyn RunSink>, root: PathBuf, host_dir: PathBuf) -> Self {
        Self { runtime, sink, root, host_dir, turns: Turns::default() }
    }

    fn record(&self, mut turn: Turn) {
        let session = find_session(&self.root, &self.host_dir, &turn.conversation);
        if turn.model.is_none() {
            turn.model = session.as_ref().and_then(|record| record.model.clone());
        }
        let run = RunRecord {
            session: session.as_ref().map_or_else(|| turn.conversation.clone(), |record| record.name.clone()),
            task_ref: session.as_ref().and_then(|record| record.task_ref.clone()),
            provider: self.runtime.provider().label().to_owned(),
            account: session.as_ref().map(|record| record.account.clone()),
            system_prompt: session.and_then(|record| record.system_prompt).and_then(|path| std::fs::read_to_string(path).ok()),
            turn,
        };
        if let Err(error) = self.sink.record_run(&run) {
            eprintln!("north ledger: {}", error.user_message());
        }
    }
}

impl FrameWatch for TurnWatch {
    fn line(&mut self, line: &[u8]) {
        let Ok(frame) = serde_json::from_slice::<Value>(line) else { return };
        for signal in self.runtime.signals(&frame) {
            if let Some(turn) = self.turns.observe(signal, now_ms()) {
                self.record(turn);
            }
        }
    }

    fn in_flight(&self) -> bool {
        self.turns.in_flight()
    }

    fn close(&mut self) {
        for turn in self.turns.abandon(now_ms()) {
            self.record(turn);
        }
    }
}
