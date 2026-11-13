//! How a watch reads its provider's frames: the questions it relays, the prompts and answers it journals,
//! and the rest, compaction or exit it asks its session host for.
use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::Value;

use super::handover;
use super::{TurnWatch, View};
use crate::fleet::claims;
use crate::fleet::gates::Question;
use crate::fleet::idle::{self, Step};
use crate::fleet::model::Phase;
use crate::fleet::records::{find_session, host_sessions};
use crate::fleet::runtime::now_ms;
use crate::fleet::session_host::{FrameWatch, Rest, SentLog};
use crate::fleet::silent::State;
use crate::fleet::waits::Waits;

impl TurnWatch {
    fn relay(&self, question: Question) {
        let Some(record) = find_session(&self.root, &self.host_dir, &question.conversation) else { return };
        for ask in &question.asks {
            self.courier.ask(ask.gate(&record.name));
        }
        self.courier.answer(&self.host_dir, question.reply);
    }
}

impl TurnWatch {
    pub fn line(&mut self, line: &[u8]) {
        let Ok(frame) = serde_json::from_slice::<Value>(line) else { return };
        if let Some(question) = self.runtime.question(&frame) {
            self.relay(question);
        }
        if let Some(id) = self.runtime.accepted_prompt(&frame) {
            self.unaccepted.journal().accepted(&id);
        }
        self.record_usage(&frame);
        let signals = self.runtime.signals(&frame);
        for signal in &signals {
            self.views.entry(signal.conversation().to_owned()).or_insert_with(|| View::of(signal.conversation()));
        }
        let answers: Vec<_> = self.views.iter_mut().map(|(conversation, view)| (conversation.clone(), view.observe(self.runtime.as_ref(), &frame))).filter(|(_, said)| !said.is_empty()).collect();
        let answers: Vec<_> = answers.into_iter().filter(|(conversation, _)| self.checkpoints.awaiting(conversation)).collect();
        for (conversation, said) in answers {
            let model = find_session(&self.root, &self.host_dir, &conversation).and_then(|record| record.model);
            self.checkpoints.answered(&conversation, &said, model.as_deref());
        }
        for signal in signals {
            self.follow(&signal);
            if let Some(turn) = self.turns.observe(signal, now_ms()) {
                self.record(turn);
            }
        }
    }

    /// Logs a frame this host queued, through a call kept outside this watch's lock.
    fn sent_log(&self) -> SentLog {
        let prompts = self.unaccepted.clone();
        SentLog::new(move |line| prompts.sent(line))
    }

    /// Hands this watch to a session host: every question the host asks reaches in through one lock, and a frame it
    /// sends is logged without taking that lock.
    pub fn watching(self) -> FrameWatch {
        let sent = self.sent_log();
        let watch = Arc::new(Mutex::new(self));
        let line = Arc::clone(&watch);
        let in_flight = Arc::clone(&watch);
        let soonest_rest = Arc::clone(&watch);
        let rest = Arc::clone(&watch);
        let close = Arc::clone(&watch);
        let hand_over = Arc::clone(&watch);
        let take_over = Arc::clone(&watch);
        FrameWatch::new(move |bytes| held(&line).line(bytes), sent)
            .in_flight_when(move || held(&in_flight).in_flight())
            .soonest_rest_when(move || held(&soonest_rest).soonest_rest())
            .rest_by(move |idle, park_after| held(&rest).rest(idle, park_after))
            .close_by(move || held(&close).close())
            .hand_over_by(move || held(&hand_over).hand_over())
            .take_over_by(move |state| held(&take_over).take_over(state))
    }

    pub fn in_flight(&self) -> bool {
        self.turns.in_flight()
    }

    pub fn soonest_rest(&self) -> Option<Duration> {
        Some(self.idle.registered_park_after)
    }

    pub fn hand_over(&mut self) -> Value {
        handover::hand_over(self)
    }

    pub fn take_over(&mut self, state: Value) {
        handover::take_over(self, state);
    }

    pub fn rest(&mut self, idle: Duration, park_after: Option<Duration>) -> Rest {
        let finished = !self.finished.is_empty() && self.finished.values().all(|finished| *finished);
        let registered = self.registered_wait();
        let session = idle::Session { idle, in_flight: self.turns.in_flight(), turn_open: self.turns.turn_open(), context: self.turns.context(), rolled_over: self.rolled_over, finished, registered };
        let step = self.idle.step(session, park_after);
        match step {
            Step::Park => {
                self.speak_up(State::Parked, idle);
            }
            _ if self.quiet.values().any(|quiet| quiet.due(idle)) => self.speak_up(State::Idle, idle),
            _ => {}
        }
        match step {
            Step::Wait => self.checkpoint_frames().map_or(Rest::Wait, Rest::Send),
            Step::Park => {
                self.park();
                Rest::End
            }
            Step::Compact => {
                self.rolled_over = true;
                self.runtime.compact(None).map_or(Rest::Wait, |frame| Rest::Send(vec![frame]))
            }
            Step::Exit if self.retire(Phase::IdleExited) => {
                self.speak_up(State::Exited, idle);
                Rest::End
            }
            Step::Exit => self.rest(idle, park_after),
        }
    }

    pub fn close(&mut self) {
        self.closing = true;
        for turn in self.turns.abandon(now_ms()) {
            self.record(turn);
        }
        let ended: HashSet<String> = host_sessions(&self.root, &self.host_dir).into_iter().filter_map(|record| record.conversation).collect();
        for conversation in &ended {
            self.recording.ended(conversation);
        }
        let retired = !self.finished.is_empty() && self.finished.values().all(|finished| *finished) && self.unaccepted.journal().is_empty() && self.retire(Phase::HostEnded);
        let waits = Waits::in_root(&self.root);
        for record in host_sessions(&self.root, &self.host_dir).into_iter().filter(|record| record.ended() || waits.of(&record.name).is_empty()) {
            claims::release_for(self.claims.as_ref(), &record);
        }
        self.finish_probes();
        self.speak_up(if retired { State::Exited } else { State::Parked }, Duration::ZERO);
        self.finish_probes();
    }
}


/// The watch's lock, taken for one call from the host and released before it asks anything else.
fn held(watch: &Mutex<TurnWatch>) -> MutexGuard<'_, TurnWatch> {
    watch.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
