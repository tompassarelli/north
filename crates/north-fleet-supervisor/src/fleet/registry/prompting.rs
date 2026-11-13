//! Driving a session's turn: the text delivered into it, and the interrupt that cuts a turn short.

use crate::error::{NorthError, NorthResult};
use crate::fleet::host::Host;
use crate::fleet::model::{Delivery, SessionRecord, Status, Transcript};
use crate::fleet::sender::urgent_prefixed;
use crate::fleet::transcript::stream_len;

use super::awaiting::await_settled;
use super::stopping::Halt;
use super::Fleet;

impl Fleet {
    pub(super) fn prompt(&self, name: &str, id: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
        let held = self.record(name)?;
        if held.adopted() {
            return self.tell_window(&held, text);
        }
        let record = self.wake(name)?;
        match self.prompt_live(&record, id, text, delivery) {
            Err(_) if Host::new(record.host_dir.clone()).closed() => {
                Host::new(record.host_dir.clone()).await_end()?;
                self.prompt_live(&self.wake(name)?, id, text, delivery)
            }
            sent => sent,
        }
    }

    pub(super) fn prompt_live(&self, record: &SessionRecord, id: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
        let transcript = self.recent(self.runtime(record)?.as_ref(), record)?;
        match delivery {
            Delivery::Now if transcript.status != Status::Idle => self.send_urgent(record, &transcript, id, text),
            _ => self.send_text(record, &transcript, id, text),
        }
    }

    pub fn interrupt(&self, name: &str) -> NorthResult<()> {
        self.interrupt_for(name, Halt::Ordered)
    }

    /// Interrupts `name`'s turn; a turn it cuts short records `why` as its cause.
    pub(super) fn interrupt_for(&self, name: &str, why: Halt) -> NorthResult<()> {
        let record = self.record(name)?;
        if record.adopted() {
            return Err(NorthError::Usage(format!("{name} is an interactive window; interrupt it there")));
        }
        let runtime = self.runtime(&record)?;
        let transcript = self.recent(runtime.as_ref(), &record)?;
        Host::new(record.host_dir.clone()).send(&runtime.interrupt(&transcript)?)?;
        if transcript.status == Status::Working {
            why.record(name);
        }
        Ok(())
    }

    pub(super) fn send_text(&self, record: &SessionRecord, transcript: &Transcript, id: &str, text: &str) -> NorthResult<()> {
        Host::new(record.host_dir.clone()).send(&self.runtime(record)?.message(transcript, id, text)?)
    }

    fn send_urgent(&self, record: &SessionRecord, transcript: &Transcript, id: &str, text: &str) -> NorthResult<()> {
        let runtime = self.runtime(record)?;
        let interrupted_from = stream_len(&record.host_dir);
        Host::new(record.host_dir.clone()).send(&runtime.interrupt(transcript)?)?;
        let settled = await_settled(runtime.as_ref(), record, transcript.clone(), interrupted_from)?;
        self.send_text(record, &settled, id, &urgent_prefixed(text))
    }
}
