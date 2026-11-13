//! The turn a session host serves, as the host sees it. The host copies a provider's output and queues its input;
//! what a copied line means, whether a turn is still in flight, when the host may rest and what its next image must
//! carry belong to the turn watcher, which lives above this crate. Each answer arrives as a call the watcher
//! supplies, so the process host stays free of the turn's parsing without declaring a trait for the one watcher.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use super::Rest;

/// The frames a host queued, logged through a call kept outside the watch's lock so a frame the host is about to
/// write never waits on a line another thread is parsing.
#[derive(Clone)]
pub struct SentLog(Arc<dyn Fn(&str) + Send + Sync>);

impl SentLog {
    pub fn new(sent: impl Fn(&str) + Send + Sync + 'static) -> Self {
        Self(Arc::new(sent))
    }

    pub fn sent(&self, line: &str) {
        (self.0)(line);
    }
}

impl Default for SentLog {
    fn default() -> Self {
        Self::new(|_| {})
    }
}

/// What a host asks its turn watcher and what it tells it. A host handed no watcher keeps these defaults: no line is
/// parsed, nothing is in flight, nothing is carried over, and it parks only where its idle limit says so.
pub struct FrameWatch {
    line: Line,
    sent: SentLog,
    in_flight: Option<Call<bool>>,
    soonest_rest: Option<Call<Option<Duration>>>,
    rest: Option<Resting>,
    close: Option<Closing>,
    hand_over: Option<Call<Value>>,
    take_over: Option<Taking>,
}

/// A call the host makes without an argument, boxed so the host holds it without naming the watcher above.
type Call<T> = Box<dyn FnMut() -> T + Send>;
/// A copied line, handed to the watcher that parses the turn's frames.
type Line = Box<dyn FnMut(&[u8]) + Send>;
/// How long the host may rest, given its idle time and its idle limit.
type Resting = Box<dyn FnMut(Duration, Option<Duration>) -> Rest + Send>;
/// The host's last words to the watcher before it winds down.
type Closing = Box<dyn FnMut() + Send>;
/// The state the host's next image must carry.
type Taking = Box<dyn FnMut(Value) + Send>;

impl Default for FrameWatch {
    fn default() -> Self {
        Self { line: Box::new(|_| {}), sent: SentLog::default(), in_flight: None, soonest_rest: None, rest: None, close: None, hand_over: None, take_over: None }
    }
}

impl FrameWatch {
    pub fn new(line: impl FnMut(&[u8]) + Send + 'static, sent: SentLog) -> Self {
        Self { line: Box::new(line), sent, ..Self::default() }
    }

    pub fn in_flight_when(mut self, ask: impl FnMut() -> bool + Send + 'static) -> Self {
        self.in_flight = Some(Box::new(ask));
        self
    }

    pub fn soonest_rest_when(mut self, ask: impl FnMut() -> Option<Duration> + Send + 'static) -> Self {
        self.soonest_rest = Some(Box::new(ask));
        self
    }

    pub fn rest_by(mut self, decide: impl FnMut(Duration, Option<Duration>) -> Rest + Send + 'static) -> Self {
        self.rest = Some(Box::new(decide));
        self
    }

    pub fn close_by(mut self, close: impl FnMut() + Send + 'static) -> Self {
        self.close = Some(Box::new(close));
        self
    }

    pub fn hand_over_by(mut self, hand: impl FnMut() -> Value + Send + 'static) -> Self {
        self.hand_over = Some(Box::new(hand));
        self
    }

    pub fn take_over_by(mut self, take: impl FnMut(Value) + Send + 'static) -> Self {
        self.take_over = Some(Box::new(take));
        self
    }

    pub fn sent_log(&self) -> SentLog {
        self.sent.clone()
    }

    pub fn line(&mut self, line: &[u8]) {
        (self.line)(line);
    }

    pub fn in_flight(&mut self) -> bool {
        self.in_flight.as_mut().is_none_or(|ask| ask())
    }

    pub fn soonest_rest(&mut self) -> Option<Duration> {
        self.soonest_rest.as_mut().and_then(|ask| ask())
    }

    pub fn rest(&mut self, idle: Duration, park_after: Option<Duration>) -> Rest {
        match self.rest.as_mut() {
            Some(decide) => decide(idle, park_after),
            None if park_after.is_some_and(|limit| idle >= limit) => Rest::End,
            None => Rest::Wait,
        }
    }

    pub fn close(&mut self) {
        if let Some(close) = self.close.as_mut() {
            close();
        }
    }

    pub fn hand_over(&mut self) -> Value {
        match self.hand_over.as_mut() {
            Some(hand) => hand(),
            None => Value::Null,
        }
    }

    pub fn take_over(&mut self, state: Value) {
        if let Some(take) = self.take_over.as_mut() {
            take(state);
        }
    }
}
