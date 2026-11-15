//! The worker's compaction checkpoint: at 250k tokens of context and every 50k after, the session host asks the worker
//! for one line, `Turns left: N. Focus: TEXT`, and compacts with that focus when the turns left repay a compaction.

use std::collections::HashMap;

use north_pace::router::pricing::PriceTable;

pub const FIRST: u64 = 250_000;
pub const STEP: u64 = 50_000;
const PRICED_AS: &str = "opus";
const REPLY_LABEL: &str = "Turns left:";
const FOCUS_LABEL: &str = "Focus:";

pub fn question(context: u64) -> String {
    format!(
        "Your context is {}k tokens. Answer with exactly one line, `Turns left: N. Focus: TEXT`, where N is how many more turns you expect before you report, \
         and TEXT is what a compaction must keep: the decisions and dead ends not yet in the repository, and the next step. \
         The session host compacts with that focus when N turns repay a compaction at this size, and otherwise asks again 50k tokens later. Then continue your work.",
        context / 1000
    )
}

/// The turn that sends a worker back to its task once the compaction its answer chose has run as a turn of its own.
pub const RESUME: &str = "The session host compacted your context at your checkpoint, and the brief, lane, branch and open boxes restated above are your task. \
     Continue it from your focus; if you were waiting on a background command or a Monitor, read its output or wait on it again. End the task with your report.";

#[derive(Debug, Eq, PartialEq)]
pub struct Answer {
    pub turns_left: u64,
    pub focus: String,
}

impl Answer {
    pub fn parse(text: &str) -> Option<Self> {
        text.lines().find_map(|line| {
            let line = line.trim().trim_matches(['*', '`', '>', ' ']);
            let rest = line.strip_prefix(REPLY_LABEL)?.trim_start();
            let (number, rest) = rest.split_once(|letter: char| !letter.is_ascii_digit()).unwrap_or((rest, ""));
            let turns_left = number.parse().ok()?;
            let focus = rest.trim_start_matches(['.', ' ']).strip_prefix(FOCUS_LABEL).map_or("", str::trim);
            Some(Self { turns_left, focus: focus.trim_matches(['*', '`']).trim().to_owned() })
        })
    }
}

fn price(prices: &PriceTable, model: Option<&str>) -> north_pace::router::pricing::Price {
    model.and_then(|model| prices.price(model)).or_else(|| prices.price(PRICED_AS)).expect("the price table prices the fallback tier")
}

/// Whether compacting at `context` repays its cost with `turns_left` turns to go, priced for `model` (Opus when unknown).
pub fn pays(model: Option<&str>, context: u64, turns_left: u64) -> bool {
    PriceTable::load().is_ok_and(|prices| price(&prices, model).compaction_pays(context, turns_left))
}

/// One conversation's checkpoint: when it last asked, whether an answer is awaited, the compaction its answer chose,
/// whether that compaction was sent, and whether the worker is owed its resume turn.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Checkpoint {
    last: Option<u64>,
    asked_at: Option<u64>,
    ready: Option<String>,
    sent: bool,
    resume: bool,
}

impl Checkpoint {
    /// Whether the worker is to be asked now, at `context`.
    pub fn due(&self, context: u64) -> bool {
        context >= FIRST && self.ready.is_none() && self.last.is_none_or(|last| context >= last + STEP)
    }

    pub fn ask(&mut self, context: u64) {
        self.last = Some(context);
        self.asked_at = Some(context);
    }

    pub fn answered(&mut self, reply: &str, model: Option<&str>) {
        let Some(context) = self.asked_at else { return };
        let Some(answer) = Answer::parse(reply) else { return };
        self.asked_at = None;
        if pays(model, context, answer.turns_left) {
            self.ready = Some(answer.focus);
        }
    }

    /// The focus of the compaction the answer chose, once.
    pub fn take_ready(&mut self) -> Option<String> {
        let focus = self.ready.take()?;
        self.sent = true;
        Some(focus)
    }

    /// A compaction ran; only one this checkpoint sent owes the worker a resume turn.
    pub fn compacted(&mut self) {
        let resume = self.sent;
        *self = Self { resume, ..Self::default() };
    }

    pub fn take_resume(&mut self) -> bool {
        std::mem::take(&mut self.resume)
    }
}

/// Every conversation's checkpoint, by conversation id.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Checkpoints(HashMap<String, Checkpoint>);

impl Checkpoints {
    pub fn due(&self, conversation: &str, context: u64) -> bool {
        self.0.get(conversation).map_or(context >= FIRST, |checkpoint| checkpoint.due(context))
    }

    pub fn ask(&mut self, conversation: &str, context: u64) {
        self.0.entry(conversation.to_owned()).or_default().ask(context);
    }

    pub fn awaiting(&self, conversation: &str) -> bool {
        self.0.get(conversation).is_some_and(|checkpoint| checkpoint.asked_at.is_some())
    }

    pub fn answered(&mut self, conversation: &str, said: &[String], model: Option<&str>) {
        if let Some(checkpoint) = self.0.get_mut(conversation) {
            said.iter().for_each(|text| checkpoint.answered(text, model));
        }
    }

    pub fn compacted(&mut self, conversation: &str) {
        if let Some(checkpoint) = self.0.get_mut(conversation) {
            checkpoint.compacted();
        }
    }

    pub fn take_ready(&mut self) -> Option<String> {
        self.0.values_mut().find_map(Checkpoint::take_ready)
    }

    pub fn take_resume(&mut self) -> bool {
        self.0.values_mut().any(Checkpoint::take_resume)
    }
}
