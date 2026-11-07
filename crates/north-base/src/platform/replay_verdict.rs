use super::trials::{Verdict as Shift, compare};

const MOST_SAMPLES: usize = 50;

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Kept,
    Regressed,
    FailsOnBoth,
    AvoidedOnBoth,
    Inconclusive(Option<usize>),
}

impl Verdict {
    pub fn line(&self) -> String {
        match self {
            Verdict::Kept => "kept".into(),
            Verdict::Regressed => "regressed".into(),
            Verdict::FailsOnBoth => "fails on both arms".into(),
            Verdict::AvoidedOnBoth => "avoided on both arms".into(),
            Verdict::Inconclusive(Some(needed)) => format!("inconclusive: {needed} samples per arm at these rates decide it"),
            Verdict::Inconclusive(None) => format!("inconclusive: at these rates no count up to {MOST_SAMPLES} samples per arm decides it"),
        }
    }
}

fn shift(base: (usize, usize), lane: (usize, usize)) -> Shift {
    let avoided = |(repeats, of): (usize, usize)| ((of - repeats) as f64 / of as f64, of);
    compare(avoided(base), avoided(lane))
}

pub fn decide(base: (usize, usize), lane: (usize, usize), deterministic: bool) -> Option<Verdict> {
    if base.1 == 0 || lane.1 == 0 {
        return None;
    }
    if deterministic {
        return Some(match (base.0 > 0, lane.0 > 0) {
            (true, false) => Verdict::Kept,
            (false, true) => Verdict::Regressed,
            (true, true) => Verdict::FailsOnBoth,
            (false, false) => Verdict::AvoidedOnBoth,
        });
    }
    Some(match shift(base, lane) {
        Shift::Gain => Verdict::Kept,
        Shift::Drop => Verdict::Regressed,
        Shift::Same => {
            let at = |n: usize, (repeats, of): (usize, usize)| (((repeats * n) as f64 / of as f64).round() as usize, n);
            Verdict::Inconclusive((2..=MOST_SAMPLES).find(|&n| shift(at(n, base), at(n, lane)) != Shift::Same))
        }
    })
}

pub const BASE_ARM: &str = "base";
pub const LANE_ARM: &str = "lane";
pub const PROOF_SUFFIX: &str = ".proof.jsonl";

/// One graded sample of a harness case replay, the line `north policy-replay case` writes to the case's proof file.
pub fn proof_row(arm: &str, generation: &str, sample: usize, repeats: Option<bool>, deterministic: bool) -> serde_json::Value {
    serde_json::json!({"arm": arm, "generation": generation, "sample": sample, "repeats": repeats, "detector": if deterministic { "pattern" } else { "judge" }})
}

/// The verdict a case's proof file reads between its `base` and `lane` arms, or `None` when either arm has no graded sample.
pub fn proven(proof: &str) -> Option<Verdict> {
    let rows: Vec<serde_json::Value> = proof.lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    let count = |arm: &str| {
        let marks: Vec<bool> = rows.iter().filter(|row| row["arm"] == arm).filter_map(|row| row["repeats"].as_bool()).collect();
        (marks.iter().filter(|mark| **mark).count(), marks.len())
    };
    let deterministic = !rows.is_empty() && rows.iter().all(|row| row["detector"] == "pattern");
    decide(count(BASE_ARM), count(LANE_ARM), deterministic)
}

#[cfg(test)]
mod tests {
    use super::{Verdict, decide};

    #[test]
    fn a_two_sample_swing_is_inconclusive_and_names_the_samples_a_decision_needs() {
        assert_eq!(decide((0, 2), (2, 2), false), Some(Verdict::Inconclusive(Some(4))));
        assert_eq!(decide((0, 4), (4, 4), false), Some(Verdict::Regressed));
        assert_eq!(decide((4, 4), (0, 4), false), Some(Verdict::Kept));
        assert_eq!(decide((2, 2), (0, 2), true), Some(Verdict::Kept));
    }
}
