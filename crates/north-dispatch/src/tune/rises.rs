//! Cells on a tier still staffed this week whose median cost of a closed box rose more than 25% week
//! on week, together as the one rolling north issue a tick keeps for them.
use crate::ledger::{ClosedBox, ClosedReport};

/// The rise over the week before past which a cell gets an issue; it must hold with the week's dearest box dropped, so one dear box moves no median past it.
pub const RISE: f64 = 0.25;
/// Closed boxes a cell needs in each week before its rise counts, so one dear box files nothing.
pub const RISE_MIN_BOXES: usize = 3;
/// The category of a box no label has named yet; its spend moves to a real cell once `threads label-runs` reads the issue, so a rise here is an artifact of the labeling lag.
const UNLABELED: &str = "unknown";

#[derive(Clone, Debug, PartialEq)]
pub struct Rise {
    pub tier: String,
    pub category: String,
    pub before: (f64, usize),
    pub after: (f64, usize),
    pub boxes: Vec<ClosedBox>,
}

pub fn rises(week: &ClosedReport, before: &ClosedReport) -> Vec<Rise> {
    week.cells
        .iter()
        .filter(|cell| cell.staffed && cell.category != UNLABELED)
        .filter_map(|cell| {
            let earlier = before.cells.iter().find(|earlier| earlier.tier == cell.tier && earlier.category == cell.category)?;
            let (was, now, steady) = (earlier.median_box_usd()?, cell.median_box_usd()?, cell.median_box_usd_without_dearest()?);
            let bar = was * (1.0 + RISE);
            (earlier.closed >= RISE_MIN_BOXES && cell.closed >= RISE_MIN_BOXES && was > 0.0 && now > bar && steady > bar).then(|| Rise {
                tier: cell.tier.clone(),
                category: cell.category.clone(),
                before: (was, earlier.closed),
                after: (now, cell.closed),
                boxes: cell.boxes_dearest_first(),
            })
        })
        .collect()
}

pub const TITLE: &str = "Cost per closed box rose more than 25% week on week";

pub fn issue_body(rises: &[Rise]) -> Option<String> {
    if rises.is_empty() {
        return None;
    }
    let sections: String = rises.iter().map(Rise::section).collect();
    Some(format!(
        "Spec: measured\nScope: cross-module\nSurface: rust\nVerify: local-test\n\n\
         `north tune` keeps this issue while any staffed cell's median cost of a closed box sits more than {:.0}% above the week before (each week holding at least {RISE_MIN_BOXES} closed boxes), and closes it on the first tick no cell has.\n\n\
         ## Done when\n- [ ] (headless run) Each rise below is traced to its runs in `north ledger summary --since 7d`, and either a fix lands or `north tune` shows the cell back within 25% of the week before.\n\n{sections}",
        RISE * 100.0,
    ))
}

impl Rise {
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

    fn section(&self) -> String {
        format!(
            "## {tier} {category}\n\nMedian cost of a closed box rose from ${:.2} over {} closed boxes the week before to ${:.2} over {} this week (+{:.0}%).\n\n{boxes}",
            self.before.0,
            self.before.1,
            self.after.0,
            self.after.1,
            self.percent(),
            tier = self.tier,
            category = self.category,
            boxes = self.boxes_paragraph(),
        )
    }

    fn boxes_paragraph(&self) -> String {
        if self.boxes.is_empty() {
            return String::new();
        }
        let lines: Vec<String> = self.boxes.iter().map(box_line).collect();
        format!("Closed boxes behind this week's figure, dearest first, each with its worker runs in the order they started (the first run's tier and the first labeled category name the cell):\n{}\n\n", lines.join("\n"))
    }
}

fn box_line(closed: &ClosedBox) -> String {
    let runs: Vec<String> = closed.runs.iter().map(|run| format!("{} {} {} from {} ${:.2}", run.agent, run.tier, run.category, run.started, run.usd)).collect();
    format!("- {} ${:.2}: {}", closed.item, closed.usd, runs.join("; "))
}
