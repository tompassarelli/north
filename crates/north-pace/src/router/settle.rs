//! The rule that settles an arm comparison. Closed-box cost is heavy-tailed (one umbrella issue moves
//! a cell), so a point difference at `ARM_RUNS` boxes would settle the dearer arm on a noisy draw for
//! good. An arm wins only when a seeded bootstrap over both arms' boxes puts it ahead on cost per kept
//! box in `AHEAD` of `RESAMPLES` resamples and its kept share is no lower; until then the arms keep
//! sampling, and each arm's share of the next boxes follows Thompson sampling on cost per kept box
//! (`shares`). The policy climber may not change this file (docs/rationale/policy-climber.md).
use crate::router::policy::ARM_RUNS;

pub const RESAMPLES: usize = 100;
pub const AHEAD: usize = 90;
const SEED: u64 = 0x5e77_1e00_a4b0_0636;
/// The share every arm keeps until its comparison settles.
pub const FLOOR: f64 = 0.1;
/// The posterior draws behind each arm's share.
pub const DRAWS: usize = 1000;
const SHARE_SEED: u64 = 0x7407_9503_a4b0_0636;

/// One box an arm ran: its dollars, whether its issue closed, and whether it closed without rework.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outcome {
    pub usd: f64,
    pub closed: bool,
    pub kept: bool,
}

/// One arm's evidence: the runs stamped with it and the boxes its first runs started.
#[derive(Clone, Copy, Debug)]
pub struct Arm<'a> {
    pub runs: usize,
    pub boxes: &'a [Outcome],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Side {
    First,
    Second,
}

pub fn closed(boxes: &[Outcome]) -> usize {
    boxes.iter().filter(|outcome| outcome.closed).count()
}

fn kept(boxes: &[Outcome]) -> usize {
    boxes.iter().filter(|outcome| outcome.kept).count()
}

/// The spend of every box, closed or not, over the kept boxes; None with no closed box, infinite when
/// every closed box was reworked.
pub fn usd_per_kept(boxes: &[Outcome]) -> Option<f64> {
    (closed(boxes) > 0).then(|| per_kept(boxes.iter().map(|outcome| outcome.usd).sum(), kept(boxes)))
}

fn per_kept(usd: f64, kept: usize) -> f64 {
    if kept == 0 { f64::INFINITY } else { usd / kept as f64 }
}

fn kept_share(boxes: &[Outcome]) -> f64 {
    kept(boxes) as f64 / closed(boxes) as f64
}

/// The arm that settles the comparison, `first` when both cost the same, or None while either arm holds
/// fewer than `ARM_RUNS` runs or closed boxes, the cheaper arm keeps a lower share of its boxes, or the
/// bootstrap does not put the cheaper arm ahead in `AHEAD` of `RESAMPLES` resamples.
pub fn settled(first: Arm, second: Arm) -> Option<Side> {
    let full = |arm: &Arm| arm.runs >= ARM_RUNS && closed(arm.boxes) >= ARM_RUNS;
    if !full(&first) || !full(&second) {
        return None;
    }
    let (side, cheaper, dearer) = if usd_per_kept(second.boxes)? < usd_per_kept(first.boxes)? { (Side::Second, second.boxes, first.boxes) } else { (Side::First, first.boxes, second.boxes) };
    if kept_share(cheaper) < kept_share(dearer) {
        return None;
    }
    let mut random = SplitMix(SEED);
    let ahead = (0..RESAMPLES).filter(|_| resampled(cheaper, &mut random) < resampled(dearer, &mut random)).count();
    (ahead >= AHEAD).then_some(side)
}

fn resampled(boxes: &[Outcome], random: &mut SplitMix) -> f64 {
    let (usd, kept) = (0..boxes.len()).map(|_| boxes[random.below(boxes.len())]).fold((0.0, 0), |(usd, kept), outcome| (usd + outcome.usd, kept + usize::from(outcome.kept)));
    per_kept(usd, kept)
}

/// A seeded SplitMix64 stream, so every draw the arm rules make repeats on the same input.
pub struct SplitMix(pub u64);

impl SplitMix {
    pub fn draw(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, bound: usize) -> usize {
        (self.draw() % bound as u64) as usize
    }
}

/// The experiment an arm belongs to: a category runs its tier, effort, handoff and context arms side by
/// side, and each splits the category's boxes on its own.
pub fn family(arm: &str) -> &'static str {
    match arm {
        "no-handoff" => "handoff",
        _ if arm.starts_with("handoff-") => "handoff",
        _ if arm.starts_with("effort-") => "effort",
        _ if arm.starts_with("context-") => "context",
        _ if arm.starts_with("climb-") => "climb",
        _ => "tier",
    }
}

/// Each arm's share of the category's next boxes and whether the category is settled: all to the
/// winner once two arms settle, else the share of `DRAWS` seeded posterior draws in which the arm is
/// cheapest per kept box, lifted to at least `FLOOR`. A posterior draw resamples the arm's boxes with
/// replacement; an arm with no kept box in a draw costs infinity, and arms tied cheapest split it.
pub fn shares(arms: &[&[Outcome]], category: &str) -> (Vec<f64>, bool) {
    if let [first, second] = arms {
        let settled = settled(Arm { runs: first.len(), boxes: first }, Arm { runs: second.len(), boxes: second });
        if let Some(side) = settled {
            return (if side == Side::First { vec![1.0, 0.0] } else { vec![0.0, 1.0] }, true);
        }
    }
    let seed = category.bytes().fold(SHARE_SEED, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    let mut random = SplitMix(seed);
    let mut wins = vec![0.0; arms.len()];
    for _ in 0..DRAWS {
        let costs: Vec<f64> = arms.iter().map(|boxes| drawn(boxes, &mut random)).collect();
        let lowest = costs.iter().copied().fold(f64::INFINITY, f64::min);
        let tied: Vec<usize> = (0..costs.len()).filter(|index| costs[*index] == lowest).collect();
        tied.iter().for_each(|index| wins[*index] += 1.0 / tied.len() as f64);
    }
    let lift = 1.0 - FLOOR * arms.len() as f64;
    (wins.into_iter().map(|won| FLOOR + lift.max(0.0) * won / DRAWS as f64).collect(), false)
}

fn drawn(boxes: &[Outcome], random: &mut SplitMix) -> f64 {
    if boxes.is_empty() {
        return f64::INFINITY;
    }
    let resampled: Vec<Outcome> = (0..boxes.len()).map(|_| boxes[random.below(boxes.len())]).collect();
    let kept = resampled.iter().filter(|outcome| outcome.kept).count();
    if kept == 0 { f64::INFINITY } else { resampled.iter().map(|outcome| outcome.usd).sum::<f64>() / kept as f64 }
}
