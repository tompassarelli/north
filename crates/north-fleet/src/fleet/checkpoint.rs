//! The worker's compaction checkpoint: at 250k tokens of context and every 50k after, the session host asks the worker
//! for one line, `Turns left: N. Focus: TEXT`, and compacts with that focus when the turns left repay a compaction.

use std::collections::HashMap;

use crate::router::pricing::PriceTable;

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

fn price(prices: &PriceTable, model: Option<&str>) -> crate::router::pricing::Price {
    model.and_then(|model| prices.price(model)).or_else(|| prices.price(PRICED_AS)).expect("the price table prices the fallback tier")
}

/// Whether compacting at `context` repays its cost with `turns_left` turns to go, priced for `model` (Opus when unknown).
pub fn pays(model: Option<&str>, context: u64, turns_left: u64) -> bool {
    PriceTable::load().is_ok_and(|prices| price(&prices, model).compaction_pays(context, turns_left))
}

/// One conversation's checkpoint: when it last asked, whether an answer is awaited, and the compaction its answer chose.
#[derive(Debug, Default)]
pub struct Checkpoint {
    last: Option<u64>,
    asked_at: Option<u64>,
    ready: Option<String>,
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
        self.ready.take()
    }

    pub fn compacted(&mut self) {
        *self = Self::default();
    }
}

/// Every conversation's checkpoint, by conversation id.
#[derive(Debug, Default)]
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_line_a_worker_writes_is_read_and_the_checkpoint_asks_at_250k_then_every_50k_and_chooses_a_compaction_only_when_the_turns_left_repay_it() {
        let focus = |turns_left, focus: &str| Some(Answer { turns_left, focus: focus.to_owned() });
        assert_eq!(Answer::parse("Turns left: 40. Focus: finish the checkpoint, then land it"), focus(40, "finish the checkpoint, then land it"));
        assert_eq!(Answer::parse("Ok.\n**Turns left: 3. Focus: land box 1**\nContinuing."), focus(3, "land box 1"));
        assert_eq!(Answer::parse("Turns left: 12"), focus(12, ""));
        assert_eq!(Answer::parse("answer with `Turns left: N. Focus: TEXT`"), None);

        assert!(!pays(Some("opus"), 250_000, 18) && pays(Some("opus"), 250_000, 19));
        assert!(!pays(Some("opus"), 300_000, 14) && pays(Some("opus"), 300_000, 15));
        assert!(!pays(Some("sonnet"), 250_000, 10) && pays(Some("sonnet"), 250_000, 11));

        let mut checkpoint = Checkpoint::default();
        assert!(!checkpoint.due(249_000) && checkpoint.due(260_000));
        checkpoint.answered("Turns left: 40. Focus: x", None);
        assert_eq!(checkpoint.take_ready(), None, "an answer nobody asked for chooses nothing");
        checkpoint.ask(260_000);
        assert!(!checkpoint.due(300_000) && checkpoint.due(310_000));
        checkpoint.answered("Turns left: 3. Focus: x", Some("claude-opus-5-5"));
        assert_eq!(checkpoint.take_ready(), None);
        checkpoint.ask(310_000);
        checkpoint.answered("Turns left: 40. Focus: keep the lane", Some("claude-opus-5-5"));
        assert!(!checkpoint.due(400_000), "a chosen compaction is unsent");
        assert_eq!(checkpoint.take_ready().as_deref(), Some("keep the lane"));
        checkpoint.compacted();
        assert!(checkpoint.due(250_000));
    }
}
