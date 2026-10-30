//! What a brief tells its worker about the box beyond its facets: what the issue says is not
//! required, what the last two tries reported, and the ETA and budget the ledger's closed boxes set
//! for the box's tier and category.
use super::tier::Route;
use crate::ledger::{self, ClosedReport, Figures, PriorReport};
use std::collections::BTreeMap;

pub(crate) const REPORT_CHARS: usize = 600;
const PRIOR_REPORTS: usize = 2;
/// The closed-box window the ETA and budget are read from.
pub(crate) const WINDOW: &str = "7d";
pub(crate) const BUDGET_TIMES: f64 = 2.0;
const UNCALIBRATED: &str = "45 min (uncalibrated)";

/// The ledger as one dispatch run reads it: the closed boxes of the window and each item's last
/// Not done or Stop reports.
#[derive(Default)]
pub(crate) struct Ledger {
    closed: Option<ClosedReport>,
    prior: BTreeMap<String, Vec<PriorReport>>,
}

impl Ledger {
    pub(crate) fn read(notes: &mut Vec<String>) -> Self {
        let closed = ledger::closed_report(WINDOW).map_err(|error| notes.push(format!("ledger summary --since {WINDOW} unread, so briefs keep the uncalibrated ETA and carry no budget: {error}"))).ok();
        let prior = ledger::prior_reports().map_err(|error| notes.push(format!("prior Not done reports unread, so briefs carry none: {error}"))).unwrap_or_default();
        Self { closed, prior }
    }

    #[cfg(test)]
    pub(crate) fn new(closed: Option<ClosedReport>, prior: BTreeMap<String, Vec<PriorReport>>) -> Self {
        Self { closed, prior }
    }

    /// What the brief for `item`, whose issue body is `body` and which was opened at `created` unix
    /// seconds, says on top of its facets; a report from before `created` was about an earlier
    /// issue that held the same item name.
    pub(crate) fn grounds(&self, item: &str, created: f64, body: &str, route: &Route) -> Grounds {
        let prior = self.prior.get(item).into_iter().flatten().filter(|prior| prior.ended > created).take(PRIOR_REPORTS).map(|prior| prior.report.clone()).collect();
        Grounds {
            figures: self.closed.as_ref().and_then(|closed| closed.figures(route.tier.family(), &route.category)),
            prior,
            not_required: not_required(body),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Grounds {
    pub figures: Option<Figures>,
    pub prior: Vec<String>,
    pub not_required: Vec<String>,
}

impl Grounds {
    /// The ETA line's value: the ledger's p75 with its n, else what the issue declares, else the
    /// uncalibrated default.
    pub(crate) fn eta(&self, declared: Option<&str>, route: &Route) -> String {
        match (&self.figures, declared) {
            (Some(figures), declared) => {
                let aside = declared.map_or_else(String::new, |declared| format!("; the issue declares {declared}"));
                format!("{:.0} min (p75 of n={} runs, {}{aside})", eta_minutes(figures), figures.runs, cell(route))
            }
            (None, Some(declared)) => declared.to_owned(),
            (None, None) => UNCALIBRATED.to_owned(),
        }
    }

    pub(crate) fn measured(&self) -> bool {
        self.figures.is_some()
    }

    pub(crate) fn budget(&self, route: &Route) -> Option<String> {
        self.figures.as_ref().map(|figures| {
            format!(
                "Budget: stop and report `Not done:` with where you are at ${:.2} API-equivalent spend, twice the cell's median cost per closed box (median ${:.2} of n={} boxes, {})",
                figures.median_usd * BUDGET_TIMES,
                figures.median_usd,
                figures.closed,
                cell(route),
            )
        })
    }

    /// The issue's Not required lines and the prior reports on it.
    pub(crate) fn trace(&self) -> Vec<String> {
        let mut lines: Vec<String> = self.not_required.iter().map(|line| format!("Not required: {line}")).collect();
        lines.extend(self.prior.iter().map(|report| format!("Prior report: {}", report.chars().take(REPORT_CHARS).collect::<String>())));
        lines
    }

    pub(crate) fn rationale(&self, route: &Route) -> String {
        let source = if self.measured() { format!("the ledger's closed boxes of the last {WINDOW}") } else { "unmeasured for this cell".to_owned() };
        format!("Why: {}; the ETA and budget are {source}", route.why)
    }
}

/// The minutes a brief's ETA gives a box in a cell with these figures.
pub(crate) fn eta_minutes(figures: &Figures) -> f64 {
    figures.p75_minutes.ceil()
}

fn cell(route: &Route) -> String {
    format!("cell {} on {}, ledger {WINDOW}", route.category, route.tier.family())
}

/// The issue's Not required lines: those under a `Not required` heading, and `Not required: TEXT` lines.
pub(crate) fn not_required(body: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut under = false;
    for line in body.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            under = line.trim_start_matches('#').trim().eq_ignore_ascii_case("not required");
        } else if under {
            let item = line.trim_start_matches(['-', '*', ' ']).trim();
            found.extend((!item.is_empty()).then(|| item.to_owned()));
        } else if let Some((name, value)) = line.trim_start_matches(['-', '*', ' ']).split_once(':')
            && name.trim().trim_matches('*').eq_ignore_ascii_case("not required")
            && !value.trim_start_matches(['*', ' ']).is_empty()
        {
            found.push(value.trim_start_matches(['*', ' ']).trim().to_owned());
        }
    }
    found
}
