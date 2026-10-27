use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::error::NorthResult;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Tokens {
    pub fn total(self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }

    pub fn context(self) -> u64 {
        self.input + self.cache_read + self.cache_write
    }

    fn since(self, earlier: Tokens) -> Tokens {
        Tokens {
            input: self.input.saturating_sub(earlier.input),
            output: self.output.saturating_sub(earlier.output),
            cache_read: self.cache_read.saturating_sub(earlier.cache_read),
            cache_write: self.cache_write.saturating_sub(earlier.cache_write),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnExit {
    Completed,
    Interrupted,
    Failed,
    Killed,
}

impl TurnExit {
    pub fn label(self) -> &'static str {
        match self {
            TurnExit::Completed => "completed",
            TurnExit::Interrupted => "interrupted",
            TurnExit::Failed => "failed",
            TurnExit::Killed => "killed",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TurnTotals {
    pub tokens: Option<Tokens>,
    pub turns: Option<u64>,
    pub wall_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TurnSignal {
    Opened { conversation: String, model: Option<String>, effort: Option<String> },
    Started { conversation: String },
    Prompt { conversation: String, text: String },
    Usage { conversation: String, total: Tokens },
    Call { conversation: String, call: String, tokens: Tokens },
    Compacted { conversation: String, context: u64 },
    Ended { conversation: String, exit: TurnExit, totals: TurnTotals },
    TaskStarted { conversation: String, task: String },
    TaskEnded { conversation: String, task: String },
}

impl TurnSignal {
    pub fn conversation(&self) -> &str {
        match self {
            TurnSignal::Opened { conversation, .. }
            | TurnSignal::Started { conversation }
            | TurnSignal::Prompt { conversation, .. }
            | TurnSignal::Usage { conversation, .. }
            | TurnSignal::Call { conversation, .. }
            | TurnSignal::Compacted { conversation, .. }
            | TurnSignal::Ended { conversation, .. }
            | TurnSignal::TaskStarted { conversation, .. }
            | TurnSignal::TaskEnded { conversation, .. } => conversation,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Turn {
    pub conversation: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub prompt: Option<String>,
    pub started_ms: u64,
    pub ended_ms: u64,
    pub wall_ms: u64,
    pub turns: u64,
    pub tokens: Tokens,
    pub exit: TurnExit,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Turns {
    threads: HashMap<String, Thread>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Thread {
    model: Option<String>,
    effort: Option<String>,
    total: Tokens,
    context: u64,
    open: Option<Open>,
    tasks: HashSet<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct Open {
    started_ms: u64,
    baseline: Tokens,
    prompt: Option<String>,
    calls: u64,
    call_tokens: HashMap<String, Tokens>,
}

impl Turns {
    pub fn observe(&mut self, signal: TurnSignal, now_ms: u64) -> Option<Turn> {
        let conversation = signal.conversation().to_owned();
        let thread = self.threads.entry(conversation.clone()).or_default();
        match signal {
            TurnSignal::Opened { model, effort, .. } => {
                thread.model = model.or(thread.model.take());
                thread.effort = effort.or(thread.effort.take());
            }
            TurnSignal::Started { .. } => {
                let baseline = thread.total;
                thread.open.get_or_insert(Open { started_ms: now_ms, baseline, prompt: None, calls: 0, call_tokens: HashMap::new() });
            }
            TurnSignal::Prompt { text, .. } => {
                if let Some(open) = thread.open.as_mut().filter(|open| open.prompt.is_none()) {
                    open.prompt = Some(text);
                }
            }
            TurnSignal::Usage { total, .. } => {
                let grew = total != thread.total;
                if let (Some(open), true) = (thread.open.as_mut(), grew) {
                    open.calls += 1;
                }
                thread.total = total;
            }
            TurnSignal::Call { call, tokens, .. } => {
                thread.context = tokens.context();
                if let Some(open) = thread.open.as_mut() {
                    open.call_tokens.insert(call, tokens);
                }
            }
            TurnSignal::Compacted { context, .. } => thread.context = context,
            TurnSignal::TaskStarted { task, .. } => {
                thread.tasks.insert(task);
            }
            TurnSignal::TaskEnded { task, .. } => {
                thread.tasks.remove(&task);
            }
            TurnSignal::Ended { exit, totals, .. } => return Some(thread.finish(conversation, exit, totals, now_ms)),
        }
        None
    }

    pub fn in_flight(&self) -> bool {
        self.threads.values().any(Thread::busy)
    }

    pub fn context(&self) -> u64 {
        self.threads.values().map(|thread| thread.context).max().unwrap_or(0)
    }

    pub fn live_tokens(&self) -> u64 {
        self.threads.values().filter_map(Thread::live_tokens).sum()
    }

    pub fn busy(&self, conversation: &str) -> bool {
        self.threads.get(conversation).is_some_and(Thread::busy)
    }

    pub fn abandon(&mut self, now_ms: u64) -> Vec<Turn> {
        self.threads
            .iter_mut()
            .filter(|(_, thread)| thread.open.is_some())
            .map(|(conversation, thread)| thread.finish(conversation.clone(), TurnExit::Killed, TurnTotals::default(), now_ms))
            .collect()
    }
}

impl Thread {
    fn busy(&self) -> bool {
        self.open.is_some() || !self.tasks.is_empty()
    }

    fn live_tokens(&self) -> Option<u64> {
        let open = self.open.as_ref()?;
        if open.call_tokens.is_empty() {
            return Some(self.total.since(open.baseline).total());
        }
        Some(open.call_tokens.values().map(|tokens| tokens.total()).sum())
    }

    fn finish(&mut self, conversation: String, exit: TurnExit, totals: TurnTotals, now_ms: u64) -> Turn {
        let open = self.open.take().unwrap_or_else(|| Open {
            started_ms: now_ms.saturating_sub(totals.wall_ms.unwrap_or(0)),
            baseline: self.total,
            prompt: None,
            calls: 0,
            call_tokens: HashMap::new(),
        });
        Turn {
            conversation,
            model: self.model.clone(),
            effort: self.effort.clone(),
            prompt: open.prompt,
            started_ms: open.started_ms,
            ended_ms: now_ms,
            wall_ms: totals.wall_ms.unwrap_or(now_ms.saturating_sub(open.started_ms)),
            turns: totals.turns.unwrap_or(open.calls),
            tokens: totals.tokens.unwrap_or(self.total.since(open.baseline)),
            exit,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunRecord {
    pub session: String,
    pub task_ref: Option<String>,
    pub provider: String,
    pub account: Option<String>,
    pub system_prompt_sha: Option<String>,
    pub deepen: bool,
    pub turn: Turn,
}

pub trait RunSink: Send + Sync {
    fn record_run(&self, run: &RunRecord) -> NorthResult<u64>;
    fn join_task(&self, task_ref: &str, session: &str) -> NorthResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: u64, output: u64) -> Tokens {
        Tokens { input, output, cache_read: 0, cache_write: 0 }
    }

    fn signal(turns: &mut Turns, signal: TurnSignal, now_ms: u64) -> Option<Turn> {
        turns.observe(signal, now_ms)
    }

    #[test]
    fn cumulative_usage_is_charged_to_the_turn_it_grew_in() {
        let mut turns = Turns::default();
        let thread = || "t".to_owned();
        signal(&mut turns, TurnSignal::Usage { conversation: thread(), total: tokens(100, 10) }, 0);
        signal(&mut turns, TurnSignal::Started { conversation: thread() }, 1_000);
        assert!(turns.in_flight());
        signal(&mut turns, TurnSignal::Usage { conversation: thread(), total: tokens(150, 12) }, 2_000);
        signal(&mut turns, TurnSignal::Usage { conversation: thread(), total: tokens(150, 12) }, 2_500);
        let ended = TurnSignal::Ended { conversation: thread(), exit: TurnExit::Completed, totals: TurnTotals::default() };
        let turn = signal(&mut turns, ended, 3_000).unwrap();
        assert_eq!((turn.tokens, turn.turns, turn.wall_ms, turn.exit), (tokens(50, 2), 1, 2_000, TurnExit::Completed));
        assert!(!turns.in_flight());
    }

    #[test]
    fn outstanding_background_tasks_keep_the_session_busy_after_its_turn_ends() {
        let mut turns = Turns::default();
        let conversation = || "c".to_owned();
        signal(&mut turns, TurnSignal::Started { conversation: conversation() }, 0);
        signal(&mut turns, TurnSignal::TaskStarted { conversation: conversation(), task: "agent".into() }, 1);
        signal(&mut turns, TurnSignal::Ended { conversation: conversation(), exit: TurnExit::Completed, totals: TurnTotals::default() }, 2);
        assert!(turns.in_flight(), "a subagent still running must hold off parking");
        signal(&mut turns, TurnSignal::TaskEnded { conversation: conversation(), task: "agent".into() }, 3);
        assert!(!turns.in_flight());
    }

    #[test]
    fn abandoned_turns_end_killed() {
        let mut turns = Turns::default();
        signal(&mut turns, TurnSignal::Started { conversation: "a".into() }, 5);
        let killed = turns.abandon(9);
        assert_eq!(killed.iter().map(|turn| (turn.exit, turn.wall_ms)).collect::<Vec<_>>(), [(TurnExit::Killed, 4)]);
        assert!(!turns.in_flight());
    }
}
