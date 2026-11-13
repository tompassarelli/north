//! What a watch records of the turns it reads: each turn charged to the account its host started with, the
//! report and promises its parent hears about, and the landings it notices.
use serde_json::Value;

use super::TurnWatch;
use crate::fleet::host::Host;
use crate::fleet::landings::{self, push_log};
use crate::fleet::model::SessionRecord;
use crate::fleet::prompt::is_deepened;
use crate::fleet::promises::{promised, resolver, Book, Promise};
use crate::fleet::receipts::{deliver_report, Report};
use crate::fleet::records::find_session;
use crate::fleet::runtime::now_ms;
use crate::fleet::rungs;
use crate::fleet::silent::Quiet;
use crate::fleet::turns::{RunRecord, Turn, TurnExit};
use crate::fleet::waits::Waits;

impl TurnWatch {
    pub(super) fn record(&mut self, mut turn: Turn) {
        let session = find_session(&self.root, &self.host_dir, &turn.conversation);
        identify(&mut turn, session.as_ref());
        let run = RunRecord {
            session: session.as_ref().map_or_else(|| turn.conversation.clone(), |record| record.name.clone()),
            task_ref: session.as_ref().and_then(|record| record.task_ref.clone()),
            provider: self.runtime.provider().label().to_owned(),
            overlay: session.as_ref().and_then(|record| record.overlay.clone()),
            account: session.as_ref().map(|record| Host::served_by(&self.host_dir).unwrap_or_else(|| record.account.clone())),
            system_prompt_sha: session.as_ref().and_then(|record| record.system_prompt_sha.clone()),
            deepen: turn.prompt.as_deref().is_some_and(is_deepened),
            turn,
        };
        let number = self.ledger.record_run(&run).map_err(|error| eprintln!("north ledger: {}", error.user_message())).ok();
        let reply = self.views.get(&run.turn.conversation).and_then(|view| view.reply.clone()).unwrap_or_default();
        let lines = self.views.get(&run.turn.conversation).map(|view| view.transcript.entries.clone()).unwrap_or_default();
        let Some(record) = session else { return };
        self.observe_landing(&record);
        rungs::escalate(self.ledger.as_ref(), self.courier.as_ref(), &record, (run.turn.exit == TurnExit::Completed).then_some(reply.as_str()), &self.verdicts, now_ms());
        if let Some(number) = number {
            if matches!(run.turn.exit, TurnExit::Completed | TurnExit::Interrupted) {
                self.quiet.insert(run.turn.conversation.clone(), Quiet::after(number, reply, run.turn.exit == TurnExit::Completed, record.eta_min, self.idle.eta_minute, lines));
            }
            self.report(&record, &run.turn, number);
        }
    }

    fn report(&mut self, record: &SessionRecord, turn: &Turn, number: u64) {
        let view = self.views.remove(&turn.conversation);
        let Some(parent) = record.parent.as_deref() else { return };
        let (reply, lines) = view.map(|view| (view.reply, view.transcript.entries)).unwrap_or_default();
        let landed = landings::landed_between(record, &landings::pushes_of(record, &push_log()), turn.started_ms, turn.ended_ms);
        if turn.exit == TurnExit::Completed {
            self.keep_promises(record, parent, turn, number, reply.as_deref().unwrap_or_default());
        }
        let waiting = self.tickets.unlanded(&record.name).is_ok_and(|tickets| !tickets.is_empty()) || !Waits::in_root(&self.root).of(&record.name).is_empty();
        let report = Report { child: record.name.clone(), turn: number, exit: turn.exit, landed, reply, waiting };
        if let Err(error) = deliver_report(self.ledger.as_ref(), self.ledger.as_ref(), self.courier.as_ref(), parent, &report, &lines, now_ms()) {
            eprintln!("north fleet: receipt to {parent} not sent: {}", error.user_message());
        }
    }

    fn keep_promises(&self, record: &SessionRecord, parent: &str, turn: &Turn, number: u64, reply: &str) {
        let mut promises = Vec::new();
        let search = std::env::var_os("PATH").unwrap_or_default();
        for claim in promised(reply, turn.ended_ms, self.reckon.as_ref(), &resolver(&record.cwd, &search)) {
            match claim.command {
                Some(command) => promises.push(Promise { session: record.name.clone(), turn: number, line: claim.line, command, when: claim.when, said_ms: turn.ended_ms, parent: parent.to_owned() }),
                None => eprintln!("north fleet: {}'s line {:?} names a time but no backticked command whose program resolves, so it holds no promise to remind its parent of", record.name, claim.line),
            }
        }
        if let Err(error) = Book::at(&self.root).keep(&promises) {
            eprintln!("north fleet: {}'s promises were not kept for the tick to remind its parent of: {}", record.name, error.user_message());
        }
    }

    pub(super) fn record_usage(&self, frame: &Value) {
        if self.runtime.quota(frame).is_none() {
            return;
        }
        let Some(session) = self.runtime.frame_conversation(frame).and_then(|conversation| find_session(&self.root, &self.host_dir, conversation)) else { return };
        self.usage.observe(self.runtime.provider().label(), &session.account, frame);
    }

    fn observe_landing(&self, record: &SessionRecord) {
        if let Err(error) = landings::observe_among(self.ledger.as_ref(), record, &landings::pushes_of(record, &push_log())) {
            eprintln!("north ledger: {}", error.user_message());
        }
    }
}

/// A turn's model and effort as observed, else as its session was launched.
fn identify(turn: &mut Turn, session: Option<&SessionRecord>) {
    if turn.model.is_none() {
        turn.model = session.and_then(|record| record.model.clone());
    }
    if turn.effort.is_none() {
        turn.effort = session.and_then(|record| record.effort).map(|effort| effort.label().to_owned());
    }
}

