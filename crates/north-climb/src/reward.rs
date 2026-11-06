//! The climber's reward: a live arm's kept boxes split into those the independent Done-when check
//! ticked (checker-verified) and those its workers ticked themselves. A brief-generation candidate
//! rewrites the briefs of the workers who tick boxes, so a gain made only of self-ticked boxes could be
//! bought by the candidate; selection counts it for nothing.
use crate::arms::Arm;

/// What one arm earned, under the rework judge whose version it was taken with.
#[derive(Clone, Debug, PartialEq)]
pub struct Reward {
    pub judge: u32,
    pub usd: f64,
    pub kept: usize,
    pub verified: usize,
}

impl Reward {
    pub fn of(arm: &Arm, judge: u32) -> Self {
        Self { judge, usd: arm.usd, kept: arm.kept, verified: arm.verified }
    }

    pub fn self_ticked(&self) -> usize {
        self.kept - self.verified
    }

    fn per(&self, boxes: usize) -> f64 {
        if boxes == 0 { f64::INFINITY } else { self.usd / boxes as f64 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Winner {
    Parent,
    Candidate,
}

/// The arm that wins: the one cheaper per checker-verified kept box that is no dearer per kept box.
/// None when neither is, so an arm ahead only on self-ticked boxes never wins. Both rewards must be
/// taken under one judge; `comparable` brings them there.
pub fn winner(parent: &Reward, candidate: &Reward) -> Option<Winner> {
    assert_eq!(parent.judge, candidate.judge, "rewards taken under different rework judges are compared only after `comparable`");
    let beats = |a: &Reward, b: &Reward| a.per(a.verified) < b.per(b.verified) && a.per(a.kept) <= b.per(b.kept);
    if beats(candidate, parent) {
        Some(Winner::Candidate)
    } else if beats(parent, candidate) {
        Some(Winner::Parent)
    } else {
        None
    }
}

/// Two rewards under one judge: a reward taken under an older judge than the other is judged again
/// with `rejudge` under the newer one before the two are compared.
pub fn comparable(a: Reward, b: Reward, rejudge: impl Fn(&Reward, u32) -> Reward) -> (Reward, Reward) {
    let judge = a.judge.max(b.judge);
    let current = |reward: Reward| if reward.judge < judge { rejudge(&reward, judge) } else { reward };
    (current(a), current(b))
}
