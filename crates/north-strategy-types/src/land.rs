//! The lander's choices, one step at a time: each line holds the observations so far and the step
//! chosen, since a cherry-pick's outcome or a probe's result is the input to the next choice.
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// One ticket picked onto one candidate, with what the pick observed, each None until observed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PickCase {
    pub base: String,
    pub branch: String,
    pub head: String,
    pub red_fix: bool,
    /// Branches of the tickets queued ahead of this one and not yet decided.
    pub earlier: Vec<String>,
    pub readable: Option<bool>,
    /// Whether `git cherry` shows a commit main lacks.
    pub unlanded: Option<bool>,
    pub picked_clean: Option<bool>,
    pub conflict_files: Option<String>,
    pub bare_main: Option<bool>,
    pub with_main: Option<bool>,
    pub queued_ahead: Option<String>,
    /// The commits the pick dropped, when it dropped any.
    pub dropped: Option<String>,
    pub generator_moved: Option<String>,
    /// The recent main commits the lane reverts.
    pub stale_reverts: Option<Vec<String>>,
    pub lane_gate: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pick {
    pub pick: PickKind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickKind {
    Included,
    Decided,
    Refused,
    Undecided,
    Deferred,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ticket {
    pub branch: String,
    pub head: String,
    pub red_fix: bool,
}

/// One candidate round: the queue it was built from, the picks, and the lander's state between rounds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchCase {
    pub base: String,
    pub tickets: Vec<Ticket>,
    pub picks: Vec<Pick>,
    /// The most lanes a candidate takes, None for no limit.
    pub limit: Option<u64>,
    pub red_only: bool,
    pub pause: Option<String>,
    /// Transient and deferred streaks keyed as the lander keys a set of lanes.
    pub strikes: BTreeMap<String, Streak>,
    pub spares: u64,
    pub rebuilds: u64,
    pub bisect_depth: u64,
    /// The lowest GitHub rate limit the broker last saw, None when it has seen none in a live window.
    #[serde(default)]
    pub github: Option<crate::github::Quota>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Streak {
    pub transient: u32,
    pub deferred: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchDecision {
    /// Branches the candidate carries.
    pub included: Vec<String>,
    /// The lanes to check instead when the set kept ending without a verdict.
    pub halve: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JudgeCase {
    pub verdict: Verdict,
    pub included: Vec<String>,
    pub base_moved: bool,
    pub streak: u32,
    /// Whether a culprit search already ran on this base.
    pub searched: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Green,
    Refused(String),
    RedMain(String),
    Transient(String),
    BrokenBase(String),
    Deferred(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Judgement {
    Land,
    Halve(u64),
    Culprit(String),
    Rebuild,
    RedMain(String),
    Hold(String),
    Paused(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaveCase {
    pub wanted: u64,
    /// Free and total offload slots, None when unread.
    pub slots: Option<(u64, u64)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaveDecision {
    pub width: u64,
}

/// One bisection step over the lanes a red main carries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheriffCase {
    pub head: String,
    /// Main's tip the search ran at, which may be past the red head.
    #[serde(default)]
    pub tip: String,
    pub failing: Vec<String>,
    /// Lane commits from the tip back to the last green landing, newest first.
    pub lanes: Vec<String>,
    /// The failures each probed lane showed, by lane index, in probe order.
    pub probes: Vec<(u64, Vec<String>)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SheriffStep {
    /// Probe the lane at this index next.
    Probe(u64),
    /// The lane at this index is the culprit.
    Culprit(u64),
    /// The probe of the lane at this index gave no result, so the search ends without a culprit.
    Unprobed(u64),
    /// Rerunning the failing steps at the tip reached no verdict, so nothing is reverted.
    NoVerdict,
    /// The failing steps pass at the tip, so nothing is reverted.
    Fixed,
    /// Main without the lane at this index passes the failing steps, so it is reverted.
    Revert(u64),
    /// The failing steps still fail without the culprit, so nothing is reverted.
    Environment,
}
