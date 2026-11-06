//! A candidate policy change: one typed edit to the dispatch policy, with the files its diff touches
//! and what the ledger projects it saves. A generator proposes it from the largest loss.
use north_dispatch::ledger::closed::{Cell, ClosedReport};
use north_strategy_types::placement;

use crate::arms::{self, Arm};
use crate::loss::Loss;

const TIER_POLICY: &str = "crates/north-dispatch/src/dispatch/tier.rs";

#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    TierRoute { category: String, from: String, to: String },
    ArmRoute { category: String, from: String, to: String, to_tier: String },
    OpenArm { category: String, arm: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub id: String,
    pub parent: String,
    pub change: Change,
    pub why: String,
    pub saving_usd: Option<f64>,
}

impl Change {
    pub fn touches(&self) -> Vec<String> {
        match self {
            Self::TierRoute { .. } | Self::ArmRoute { .. } | Self::OpenArm { .. } => vec![TIER_POLICY.to_owned()],
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::TierRoute { category, from, to } => format!("route category {category} from tier {from} to tier {to}"),
            Self::ArmRoute { category, from, to, .. } => format!("route category {category} off arm {from} onto arm {to}"),
            Self::OpenArm { category, arm } => format!("open arm {arm} on the boxes of category {category} that pass their run cap"),
        }
    }

    /// Applies the change to a logged placement case; true when the case is one the change reaches.
    pub fn patch(&self, case: &mut placement::Case) -> bool {
        match self {
            Self::TierRoute { category, to, .. } => reach(case, category, None, to),
            Self::ArmRoute { category, from, to_tier, .. } => reach(case, category, Some(from), to_tier),
            Self::OpenArm { category, arm } => {
                let named = reaches(case, category, None);
                if named {
                    case.brief.push_str(&format!("\nArm: {arm}"));
                }
                named
            }
        }
    }
}

fn reaches(case: &placement::Case, category: &str, arm: Option<&str>) -> bool {
    let names = |key: &str, value: &str| case.brief.lines().any(|line| line.trim().strip_prefix(key).is_some_and(|rest| rest.trim() == value));
    names("Category:", category) && arm.is_none_or(|arm| names("Arm:", arm))
}

fn reach(case: &mut placement::Case, category: &str, arm: Option<&str>, tier: &str) -> bool {
    let named = reaches(case, category, arm);
    if named {
        case.launch.overrides.tier = Some(tier.to_owned());
    }
    named
}

/// Routes the loss's category to the cell of the same category that closed boxes cheaper, with no
/// lower share of its boxes kept, once that cell has closed `min_closed` boxes.
pub struct Cheapest {
    pub min_closed: usize,
}

fn kept_share(cell: &Cell) -> f64 {
    cell.kept() as f64 / cell.closed as f64
}

impl Cheapest {
    pub fn propose(&self, report: &ClosedReport, loss: &Loss) -> Option<Candidate> {
        self.tier(report, loss).or_else(|| self.arm(report, loss)).or_else(|| Some(open(loss)))
    }

    fn tier(&self, report: &ClosedReport, loss: &Loss) -> Option<Candidate> {
        let here = report.cells.iter().find(|cell| cell.tier == loss.tier && cell.category == loss.category)?;
        let better = report
            .cells
            .iter()
            .filter(|cell| cell.category == loss.category && cell.tier != loss.tier && cell.closed >= self.min_closed)
            .filter(|cell| cell.usd_per_closed() < here.usd_per_closed() && kept_share(cell) >= kept_share(here))
            .min_by(|a, b| a.usd_per_closed().total_cmp(&b.usd_per_closed()))?;
        let change = Change::TierRoute { category: loss.category.clone(), from: loss.tier.clone(), to: better.tier.clone() };
        Some(Candidate {
            id: format!("tier-route-{}-{}", loss.category, better.tier),
            parent: "current".to_owned(),
            why: format!(
                "{} closed {} boxes at ${:.2} each with {:.0}% kept, against ${:.2} each with {:.0}% kept here",
                better.tier,
                better.closed,
                better.usd_per_closed(),
                100.0 * kept_share(better),
                here.usd_per_closed(),
                100.0 * kept_share(here)
            ),
            saving_usd: Some(here.closed as f64 * (here.usd_per_closed() - better.usd_per_closed())),
            change,
        })
    }
}

impl Cheapest {
    /// With no cheaper tier, moves the category off its dearest arm on the loss's tier onto the arm of the category with the lowest cost per kept box.
    fn arm(&self, report: &ClosedReport, loss: &Loss) -> Option<Candidate> {
        let arms: Vec<Arm> = arms::live(report).into_iter().filter(|arm| arm.category == loss.category && arm.closed >= self.min_closed).collect();
        let cost = |arm: &Arm| arm.usd_per_kept().unwrap_or(f64::INFINITY);
        let worst = arms.iter().filter(|arm| arm.tier == loss.tier).max_by(|a, b| cost(a).total_cmp(&cost(b)))?;
        let best = arms.iter().filter(|arm| arm.arm != worst.arm).min_by(|a, b| cost(a).total_cmp(&cost(b)))?;
        let share = |arm: &Arm| arm.kept as f64 / arm.closed as f64;
        (cost(best) < cost(worst) && share(best) >= share(worst)).then(|| Candidate {
            id: format!("arm-route-{}-{}-{}", loss.category, worst.arm, best.arm),
            parent: "current".to_owned(),
            why: format!("arm {} kept {} of {} boxes at ${:.2} each, against ${:.2} each with {} of {} kept on arm {}", best.arm, best.kept, best.closed, cost(best), cost(worst), worst.kept, worst.closed, worst.arm),
            saving_usd: Some(worst.kept as f64 * (cost(worst) - cost(best))),
            change: Change::ArmRoute { category: loss.category.clone(), from: worst.arm.clone(), to: best.arm.clone(), to_tier: best.tier.clone() },
        })
    }
}

/// With no cheaper tier or arm in the ledger, tries a new arm on the loss's category: `split` when unclosed boxes are most of the loss, `escalate` when repeat runs are.
fn open(loss: &Loss) -> Candidate {
    let arm = if loss.unclosed_usd >= loss.repeat_usd { "split" } else { "escalate" };
    Candidate {
        id: format!("open-arm-{}-{arm}", loss.category),
        parent: "current".to_owned(),
        why: format!("the ledger holds no cheaper tier or arm for {}, and ${:.2} of its ${:.2} loss is {}", loss.category, loss.unclosed_usd.max(loss.repeat_usd), loss.usd(), if arm == "split" { "boxes that never closed" } else { "repeat runs" }),
        saving_usd: None,
        change: Change::OpenArm { category: loss.category.clone(), arm: arm.to_owned() },
    }
}
