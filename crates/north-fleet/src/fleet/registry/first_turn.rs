use std::fs::OpenOptions;
use std::io::Write;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::error::{NorthError, NorthResult};
use crate::fleet::host::{Host, alive};
use crate::fleet::model::{Account, SessionRecord};
use crate::fleet::runtime::{CredentialFailure, Opening, new_uuid, now_ms};
use crate::fleet::transcript::{StreamTail, stream_len, stream_path};
use crate::fleet::turns::TurnSignal;

use super::Fleet;

const FIRST_TURN_WAIT: Duration = Duration::from_secs(120);
const FIRST_TURN_POLL: Duration = Duration::from_millis(100);
const CREDENTIAL_FAILURES: &str = ".credential-failures.jsonl";

#[derive(Debug, Eq, PartialEq)]
enum FirstTurn {
    Answered,
    Failed(CredentialFailure),
}

impl Fleet {
    /// Delivers the launch prompt and waits for its turn to start; a session that does not start it is stopped, so it holds no claim.
    pub(super) fn deliver_first_turn(&self, record: &mut SessionRecord, account: &Account, prompt: &str) -> NorthResult<()> {
        let failed = self.answer_first_turn(record, account, prompt);
        if failed.is_err()
            && let Err(error) = self.stop(&record.name)
        {
            eprintln!("north fleet: {} did not stop after its failed start: {}", record.name, error.user_message());
        }
        failed
    }

    fn answer_first_turn(&self, record: &mut SessionRecord, account: &Account, prompt: &str) -> NorthResult<()> {
        let FirstTurn::Failed(failure) = self.first_turn(record, prompt)? else { return Ok(()) };
        self.note_credential_failure(record, failure)?;
        if failure == CredentialFailure::Expired || self.host_in_use(record)? {
            return Err(refusal(record, failure));
        }
        Host::new(record.host_dir.clone()).terminate()?;
        record.conversation = self.runtime(record)?.assign_conversation()?;
        let conversation = record.conversation.clone();
        self.start(record, account, Opening::Fresh(conversation.as_deref()))?;
        match self.first_turn(record, prompt)? {
            FirstTurn::Answered => Ok(()),
            FirstTurn::Failed(failure) => {
                self.note_credential_failure(record, failure)?;
                Err(refusal(record, failure))
            }
        }
    }

    fn first_turn(&self, record: &SessionRecord, prompt: &str) -> NorthResult<FirstTurn> {
        let runtime = self.runtime(record)?;
        let mut tail = StreamTail::open(stream_path(&record.host_dir), stream_len(&record.host_dir));
        self.send_text(record, &self.recent(runtime.as_ref(), record)?, &new_uuid()?, prompt)?;
        let deadline = Instant::now() + FIRST_TURN_WAIT;
        let mut prompted = false;
        loop {
            for frame in tail.frames()? {
                if let Some(failure) = runtime.credential_failure(&frame) {
                    return Ok(FirstTurn::Failed(failure));
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

    fn note_credential_failure(&self, record: &SessionRecord, failure: CredentialFailure) -> NorthResult<()> {
        let failure = match failure {
            CredentialFailure::SignedOut => "signed-out",
            CredentialFailure::Expired => "oauth-expired",
        };
        let row = json!({"at_ms": now_ms(), "session": record.name, "provider": record.provider.label(), "account": record.account, "failure": failure});
        let mut log = OpenOptions::new().create(true).append(true).open(self.root.join(CREDENTIAL_FAILURES))?;
        writeln!(log, "{row}")?;
        Ok(())
    }
}

fn refusal(record: &SessionRecord, failure: CredentialFailure) -> NorthError {
    let (name, account) = (&record.name, format!("{}:{}", record.provider.label(), record.account));
    NorthError::Refused(match failure {
        CredentialFailure::SignedOut => format!("{name} on {account} answered its first turn with 'Not logged in' after one restart"),
        CredentialFailure::Expired => format!(
            "{name} on {account} answered its first turn with 'OAuth session expired': {account}'s login on {} was revoked, most likely by a copy refreshing on another host; sign it in again here and keep it in this host's profiles only",
            crate::platform::host::name()
        ),
    })
}
