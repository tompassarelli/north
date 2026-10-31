//! Cells on a tier still staffed this week whose cost per closed box rose more than 25% week on
//! week, each as the north issue a tick files for it.
use crate::ledger::ClosedReport;

/// The rise over the week before past which a cell gets an issue.
pub const RISE: f64 = 0.25;
/// Closed boxes a cell needs in each week before its rise counts, so one dear box files nothing.
pub const RISE_MIN_BOXES: usize = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Rise {
    pub tier: String,
    pub category: String,
    pub before: (f64, usize),
    pub after: (f64, usize),
}

pub fn rises(week: &ClosedReport, before: &ClosedReport) -> Vec<Rise> {
    week.cells
        .iter()
        .filter(|cell| cell.staffed)
        .filter_map(|cell| {
            let earlier = before.cells.iter().find(|earlier| earlier.tier == cell.tier && earlier.category == cell.category)?;
            let (was, now) = (earlier.usd_per_closed(), cell.usd_per_closed());
            (earlier.closed >= RISE_MIN_BOXES && cell.closed >= RISE_MIN_BOXES && was > 0.0 && now > was * (1.0 + RISE)).then(|| Rise {
                tier: cell.tier.clone(),
                category: cell.category.clone(),
                before: (was, earlier.closed),
                after: (now, cell.closed),
            })
        })
        .collect()
}

impl Rise {
    /// The issue title, the same every day the cell stays risen so an open issue holds it.
    pub fn title(&self) -> String {
        format!("Cost per closed box rose more than 25% week on week in {} {}", self.tier, self.category)
    }

    fn percent(&self) -> f64 {
        (self.after.0 / self.before.0 - 1.0) * 100.0
    }

    pub fn line(&self) -> String {
        format!(
            "rose {} {}: ${:.2} (n {}) the week before -> ${:.2} (n {}) this week, +{:.0}%",
            self.tier,
            self.category,
            self.before.0,
            self.before.1,
            self.after.0,
            self.after.1,
            self.percent()
        )
    }

    pub fn body(&self, at: &str) -> String {
        format!(
            "Category: tooling\nSpec: measured\nScope: cross-module\nSurface: rust\nVerify: local-test\n\n\
             `north tune` on {at}: cost per closed box in {tier} {category} rose from ${:.2} over {} closed boxes the week before to ${:.2} over {} this week (+{:.0}%), past the {:.0}% bar (each week holds at least {RISE_MIN_BOXES} closed boxes).\n\n\
             ## Done when\n- [ ] (headless run) The rise is traced to its runs in `north ledger summary --since 7d`, and either a fix lands or `north tune` shows the cell back within 25% of the week before.\n",
            self.before.0,
            self.before.1,
            self.after.0,
            self.after.1,
            self.percent(),
            RISE * 100.0,
            tier = self.tier,
            category = self.category,
        )
    }
}
