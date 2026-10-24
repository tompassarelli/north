use std::fs::OpenOptions;
use std::io::Write;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::error::{NorthError, NorthResult};
use crate::fleet::host::{Host, alive};
use crate::fleet::model::{Account, SessionRecord};
use crate::fleet::runtime::{Opening, now_ms};
use crate::fleet::transcript::{StreamTail, stream_len, stream_path};
use crate::fleet::turns::TurnSignal;

use super::Fleet;

const FIRST_TURN_WAIT: Duration = Duration::from_secs(120);
const FIRST_TURN_POLL: Duration = Duration::from_millis(100);
const SIGNED_OUT: &str = ".signed-out-launches.jsonl";

#[derive(Debug, Eq, PartialEq)]
enum FirstTurn {
    Answered,
    SignedOut,
}

impl Fleet {
    pub(super) fn deliver_first_turn(&self, record: &mut SessionRecord, account: &Account, prompt: &str) -> NorthResult<()> {
        if self.first_turn(record, prompt)? == FirstTurn::Answered {
            return Ok(());
        }
        self.note_signed_out(record)?;
        if self.host_in_use(record)? {
            return Err(signed_out(record));
        }
        Host::new(record.host_dir.clone()).terminate()?;
        record.conversation = self.runtime(record)?.assign_conversation()?;
        let conversation = record.conversation.clone();
        self.start(record, account, Opening::Fresh(conversation.as_deref()))?;
        match self.first_turn(record, prompt)? {
            FirstTurn::Answered => Ok(()),
            FirstTurn::SignedOut => {
                self.note_signed_out(record)?;
                Err(signed_out(record))
            }
        }
    }

    fn first_turn(&self, record: &SessionRecord, prompt: &str) -> NorthResult<FirstTurn> {
        let runtime = self.runtime(record)?;
        let mut tail = StreamTail::open(stream_path(&record.host_dir), stream_len(&record.host_dir));
        self.send_text(record, &self.recent(runtime.as_ref(), record)?, prompt)?;
        let deadline = Instant::now() + FIRST_TURN_WAIT;
        let mut prompted = false;
        loop {
            for frame in tail.frames()? {
                if runtime.signed_out(&frame) {
                    return Ok(FirstTurn::SignedOut);
                }
                let mine = runtime.signals(&frame).into_iter().filter(|signal| record.conversation.as_deref().is_none_or(|conversation| signal.conversation() == conversation));
                for signal in mine {
                    match signal {
                        TurnSignal::Prompt { .. } => prompted = true,
                        _ if prompted => return Ok(FirstTurn::Answered),
                        _ => {}
                    }
                }
            }
            if !record.host_pid.is_some_and(alive) {
                return Err(NorthError::Refused(format!("{} exited before its first turn started; see north fleet show {}", record.name, record.name)));
            }
            if Instant::now() > deadline {
                return Err(NorthError::Refused(format!("{} did not start its first turn within {} s; see north fleet show {}", record.name, FIRST_TURN_WAIT.as_secs(), record.name)));
            }
            std::thread::sleep(FIRST_TURN_POLL);
        }
    }

    fn note_signed_out(&self, record: &SessionRecord) -> NorthResult<()> {
        let row = json!({"at_ms": now_ms(), "session": record.name, "provider": record.provider.label(), "account": record.account});
        let mut log = OpenOptions::new().create(true).append(true).open(self.root.join(SIGNED_OUT))?;
        writeln!(log, "{row}")?;
        Ok(())
    }
}

fn signed_out(record: &SessionRecord) -> NorthError {
    NorthError::Refused(format!("{} on {}:{} answered its first turn with 'Not logged in' after one restart", record.name, record.provider.label(), record.account))
}
