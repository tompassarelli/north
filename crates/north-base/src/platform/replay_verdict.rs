use super::trials::{Verdict as Shift, compare};

const MOST_SAMPLES: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Bad,
    Guard,
    Control,
}

impl Kind {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "bad" => Some(Kind::Bad),
            "guard" => Some(Kind::Guard),
            "control" => Some(Kind::Control),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Kind::Bad => "bad",
            Kind::Guard => "guard",
            Kind::Control => "control",
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Kept,
    Held,
    Regressed,
    FailsOnBoth,
    AvoidedOnBoth,
    Inconclusive(Option<usize>),
}

impl Verdict {
    pub fn line(&self) -> String {
        match self {
            Verdict::Kept => "kept".into(),
            Verdict::Held => "held: the lane avoids the behavior at least as often as the base".into(),
            Verdict::Regressed => "regressed".into(),
            Verdict::FailsOnBoth => "fails on both arms".into(),
            Verdict::AvoidedOnBoth => "avoided on both arms".into(),
            Verdict::Inconclusive(Some(needed)) => format!("inconclusive: {needed} samples per arm at these rates decide it"),
            Verdict::Inconclusive(None) => format!("inconclusive: at these rates no count up to {MOST_SAMPLES} samples per arm decides it"),
        }
    }

    pub fn refuses(&self) -> bool {
        *self == Verdict::Regressed
    }
}

fn shift(base: (usize, usize), lane: (usize, usize)) -> Shift {
    let avoided = |(repeats, of): (usize, usize)| ((of - repeats) as f64 / of as f64, of);
    compare(avoided(base), avoided(lane))
}

fn needed(base: (usize, usize), lane: (usize, usize)) -> Verdict {
    let at = |n: usize, (repeats, of): (usize, usize)| (((repeats * n) as f64 / of as f64).round() as usize, n);
    Verdict::Inconclusive((2..=MOST_SAMPLES).find(|&n| shift(at(n, base), at(n, lane)) != Shift::Same))
}

pub fn decide(kind: Kind, base: (usize, usize), lane: (usize, usize), deterministic: bool) -> Option<Verdict> {
    if base.1 == 0 || lane.1 == 0 {
        return None;
    }
    if deterministic {
        return Some(match (kind, base.0 > 0, lane.0 > 0) {
            (_, false, true) => Verdict::Regressed,
            (_, true, true) => Verdict::FailsOnBoth,
            (Kind::Bad, true, false) => Verdict::Kept,
            (Kind::Bad, false, false) => Verdict::AvoidedOnBoth,
            (Kind::Guard | Kind::Control, _, false) => Verdict::Held,
        });
    }
    let holds = || (lane.0 as f64 / lane.1 as f64) <= (base.0 as f64 / base.1 as f64);
    Some(match (kind, shift(base, lane)) {
        (_, Shift::Drop) => Verdict::Regressed,
        (Kind::Bad, Shift::Gain) => Verdict::Kept,
        (Kind::Guard | Kind::Control, Shift::Gain) => Verdict::Held,
        (Kind::Guard | Kind::Control, Shift::Same) if holds() => Verdict::Held,
        (_, Shift::Same) => needed(base, lane),
    })
}

pub const BASE_ARM: &str = "base";
pub const LANE_ARM: &str = "lane";
pub const PROOF_SUFFIX: &str = ".proof.jsonl";

/// One graded sample of a harness case replay, the line `north policy-replay case` writes to the case's proof file.
pub fn proof_row(kind: Kind, arm: &str, generation: &str, sample: usize, repeats: Option<bool>, deterministic: bool) -> serde_json::Value {
    serde_json::json!({"kind": kind.word(), "arm": arm, "generation": generation, "sample": sample, "repeats": repeats, "detector": if deterministic { "pattern" } else { "judge" }})
}

/// The verdict a case's proof file reads between its `base` and `lane` arms, or `None` when either arm has no graded sample.
pub fn proven(proof: &str) -> Option<Verdict> {
    let rows: Vec<serde_json::Value> = proof.lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    let count = |arm: &str| {
        let marks: Vec<bool> = rows.iter().filter(|row| row["arm"] == arm).filter_map(|row| row["repeats"].as_bool()).collect();
        (marks.iter().filter(|mark| **mark).count(), marks.len())
    };
    let kind = rows.iter().find_map(|row| row["kind"].as_str().and_then(Kind::parse)).unwrap_or(Kind::Bad);
    let deterministic = !rows.is_empty() && rows.iter().all(|row| row["detector"] == "pattern");
    decide(kind, count(BASE_ARM), count(LANE_ARM), deterministic)
}

#[cfg(test)]
mod tests {
    use super::{Kind, Verdict, decide};

    #[test]
    fn a_two_sample_swing_is_inconclusive_and_names_the_samples_a_decision_needs() {
        assert_eq!(decide(Kind::Bad, (0, 2), (2, 2), false), Some(Verdict::Inconclusive(Some(4))));
        assert_eq!(decide(Kind::Bad, (0, 4), (4, 4), false), Some(Verdict::Regressed));
        assert_eq!(decide(Kind::Bad, (4, 4), (0, 4), false), Some(Verdict::Kept));
        assert_eq!(decide(Kind::Bad, (2, 2), (0, 2), true), Some(Verdict::Kept));
    }
}
