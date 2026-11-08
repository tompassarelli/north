//! What a child's end-of-turn report tells its parent: upward messages are by exception, because the
//! child holds more of its box than its lead. A report goes up when it opens Done, Split or Blocked,
//! lands a commit, holds an open gate, or ends while the child waits on nothing the fleet records, and
//! only when those facts differ from the last report that went up; a turn that ends waiting on its
//! own queued ticket or registered wait, or that says it stops on a landing or a clock, reaches nobody.
//! The child's own words can only silence a report, never move a harness number, so they are a hint
//! (harness/agent-text-reads.toml); every other fact is mechanical: the landed sha from the push log, open gates from the ledger, tickets from the land
//! queue and waits from the wait registry (docs/rationale/attribution.md).
use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

const SHA_CHARS: usize = 7;
const OPENINGS: [&str; 5] = ["Done", "Not done", "Split", "Stop", "Blocked"];
const TOLD: &str = "told:";

static WAITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^[\W_]*Stop:[*_` \t]*(?:landing|wait)\b").expect("waiting stop pattern"));

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Facts {
    opening: Option<&'static str>,
    landed: Option<String>,
    gates: BTreeSet<i64>,
    waiting: bool,
}

impl Facts {
    pub const PREFIX: &'static str = TOLD;

    pub fn of(reply: &str, landed: Option<&str>, gates: BTreeSet<i64>, queued: bool) -> Self {
        let waiting = queued || WAITS.is_match(reply);
        Self { opening: OPENINGS.into_iter().find(|word| opens(reply, word)), landed: landed.map(|sha| sha[..SHA_CHARS.min(sha.len())].to_owned()), gates, waiting }
    }

    /// Whether the parent has anything to act on: a Done, Split or Blocked, a landing, an open gate, or a stop while the child holds no queued ticket and no registered wait and does not itself say it waits on a landing or a clock.
    pub fn worth_telling(&self) -> bool {
        matches!(self.opening, Some("Done" | "Split" | "Blocked")) || self.landed.is_some() || !self.gates.is_empty() || !self.waiting
    }

    /// The receipt key recording that these facts went up on `turn`; its tail is what the next report is compared with.
    pub fn receipt(&self, turn: u64) -> String {
        format!("{TOLD}{turn}:{}", self.digest())
    }

    /// Whether `receipt`, the last one recorded by [`Facts::receipt`], told the parent these same facts.
    pub fn repeats(&self, receipt: Option<&str>) -> bool {
        receipt.and_then(|key| key.strip_prefix(TOLD)).and_then(|rest| rest.split_once(':')).is_some_and(|(_, digest)| digest == self.digest())
    }

    fn digest(&self) -> String {
        let text = format!("{:?}|{:?}|{:?}|{}", self.opening, self.landed, self.gates, self.waiting);
        format!("{:016x}", text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)))
    }
}

pub fn opens(reply: &str, word: &str) -> bool {
    let opening = reply.trim_start_matches(['*', '#', '`', ' ']);
    opening.strip_prefix(word).is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()))
}

/// Whether the reply opens with one of the report words a turn ends on.
pub fn is_final(reply: &str) -> bool {
    OPENINGS.iter().any(|word| opens(reply, word))
}
