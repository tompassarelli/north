//! What a brief tells its worker about the box beyond its facets: what the issue says is not
//! required, what the last two tries reported, and the ETA and budget north tune's cells set for the
//! box's tier family and category, else the ledger's closed boxes live, the ETA scaled by the tier's
//! walk-forward `eta_pred` calibration and the budget never below the window's median closed box.
use super::tier::{Route, Tier};
use super::tuned::{Figure, Tuned};
use north_graph_loops::skill::{self, Decision};
use crate::ledger::{self, ClosedReport, Figures, PriorReport, RunEnd, RunTime};
use crate::threads::calibration::scaled;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const REPORT_CHARS: usize = 600;
const ABSENT: &str = "[absent on this host]";
const BRACKETS: &str = "\"'`(<[{)]}>,;:.";
const PRIOR_REPORTS: usize = 2;
/// The closed-box window the ETA and budget are read from.
pub const WINDOW: &str = "7d";
pub const BUDGET_TIMES: f64 = 2.0;
const UNCALIBRATED: &str = "45 min (uncalibrated)";
const READ_SPEND: &str = "`north fleet spend --self` prints your API-equivalent spend so far";
const AT_THE_LINE: &str = "commit and land any change already passing its checks, then report `Split:` with the remaining parts as boxes; never stop with passing work unlanded";

/// The ledger as one dispatch run reads it: the closed boxes of the window and each item's last
/// Not done or Stop reports, the cells north tune last wrote, and each tier's calibrated ETA scale.
#[derive(Default)]
pub struct Ledger {
    closed: Option<ClosedReport>,
    prior: BTreeMap<String, Vec<PriorReport>>,
    times: BTreeMap<String, RunTime>,
    ends: BTreeMap<String, RunEnd>,
    tuned: Tuned,
    set_aside: SetAside,
    eta_scales: BTreeMap<Tier, EtaScale>,
    /// This host's home: a carried report's path under it that is gone here is marked, so no worker is sent to it.
    home: Option<PathBuf>,
}

/// A tier's walk-forward `eta_pred` calibration: the factor that brings the measured ETA to the share
/// of runs it targets, and the label naming its cell, n and median.
#[derive(Clone, Debug, PartialEq)]
pub struct EtaScale {
    pub factor: f64,
    pub label: String,
}

impl Ledger {
    pub fn read(notes: &mut Vec<String>) -> Self {
        Self::over(ledger::closed_report(WINDOW), notes)
    }

    pub fn recorded(notes: &mut Vec<String>) -> Self {
        Self::over(ledger::recorded_closed_report(WINDOW), notes)
    }

    fn over(closed: crate::error::NorthResult<ClosedReport>, notes: &mut Vec<String>) -> Self {
        let closed = closed.map_err(|error| notes.push(format!("the closed-box report of the last {WINDOW} unread, so briefs keep the uncalibrated ETA and carry no budget: {error}"))).ok();
        let prior = ledger::prior_reports().map_err(|error| notes.push(format!("prior Not done reports unread, so briefs carry none: {error}"))).unwrap_or_default();
        let times = ledger::run_times().map_err(|error| notes.push(format!("run ETAs unread, so no run past twice its ETA climbs: {error}"))).unwrap_or_default();
        let ends = ledger::run_ends().map_err(|error| notes.push(format!("run ends unread, so no run climbs on a not-done end: {error}"))).unwrap_or_default();
        let mut eta_scales = BTreeMap::new();
        for tier in [Tier::Haiku, Tier::Sonnet, Tier::Opus] {
            match ledger::eta_calibration(tier.key()) {
                Ok((Some(factor), label)) => _ = eta_scales.insert(tier, EtaScale { factor, label }),
                Ok((None, _)) => {}
                Err(error) => notes.push(format!("eta_pred calibration of {} unread, so its briefs keep the unscaled ETA: {error}", tier.key())),
            }
        }
        let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).map(PathBuf::from);
        Self { closed, prior, times, ends, tuned: Tuned::current(), set_aside: SetAside::current(), eta_scales, home }
    }

    #[cfg(test)]
    pub fn new(closed: Option<ClosedReport>, prior: BTreeMap<String, Vec<PriorReport>>) -> Self {
        Self { closed, prior, ..Self::default() }
    }

    /// The wall minutes and stated ETA the ledger holds for `session`'s run.
    pub fn time(&self, session: &str) -> Option<RunTime> {
        self.times.get(session).copied()
    }

    /// How the ledger settled `session`'s newest run.
    pub fn end(&self, session: &str) -> Option<RunEnd> {
        self.ends.get(session).copied()
    }

    #[cfg(test)]
    pub fn eta_scale(mut self, tier: Tier, scale: EtaScale) -> Self {
        self.eta_scales.insert(tier, scale);
        self
    }

    #[cfg(test)]
    pub fn tuned(self, tuned: Tuned) -> Self {
        Self { tuned, ..self }
    }

    /// What the brief for `item`, whose issue body is `body` and which was opened at `created` unix
    /// seconds, says on top of its facets; a report from before `created` was about an earlier
    /// issue that held the same item name.
    pub fn grounds(&self, item: &str, created: f64, body: &str, route: &Route, family: &'static str) -> Grounds {
        let prior = self.prior.get(item).into_iter().flatten().filter(|prior| prior.ended > created).take(PRIOR_REPORTS).map(|prior| prior.report.clone()).collect();
        Grounds {
            figures: self.closed.as_ref().and_then(|closed| closed.figures(family, &route.category)),
            eta_cell: self.tuned.figure("eta_minutes", family, route),
            budget_cell: self.tuned.figure("budget_usd", family, route),
            eta_scale: self.eta_scales.get(&route.tier).filter(|_| route.tier.family() == family).cloned(),
            family,
            floor: self.closed.as_ref().and_then(ClosedReport::median_box_usd),
            set_aside: self.set_aside.clone(),
            prior,
            not_required: not_required(body),
            home: self.home.clone(),
        }
    }
}

/// A constant the skill switch puts in place of a learned value, and the one line saying why.
#[derive(Clone, Debug, PartialEq)]
pub struct Constant {
    pub value: f64,
    pub why: String,
}

/// The decisions of a brief whose learned value the skill switch set aside, each with the constant it reads instead.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetAside {
    pub eta: Option<Constant>,
    pub budget: Option<Constant>,
    pub cost: Option<Constant>,
}

impl SetAside {
    pub fn current() -> Self {
        let read = |decision| {
            let verdict = skill::verdict(decision);
            Some(Constant { value: verdict.constant()?, why: verdict.line()? })
        };
        Self { eta: read(Decision::Eta), budget: read(Decision::Budget), cost: read(Decision::Cost) }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grounds {
    /// The model family whose cells size the box: its tier's, or fable for a hand-named fable model.
    pub family: &'static str,
    pub figures: Option<Figures>,
    /// North tune's `eta_minutes` and `budget_usd` cells for the box's tier family and category.
    pub eta_cell: Option<Figure>,
    pub budget_cell: Option<Figure>,
    /// The tier's walk-forward calibration the measured ETA is multiplied by.
    pub eta_scale: Option<EtaScale>,
    /// The median cost of a closed box across every cell of the window, and how many closed: what
    /// one real box needs, however little a cell's own few boxes cost.
    pub floor: Option<(f64, usize)>,
    /// The constants the skill switch puts in place of a learned ETA, budget or cost that loses to them.
    pub set_aside: SetAside,
    pub prior: Vec<String>,
    pub not_required: Vec<String>,
    /// This host's home, whose absent paths in a carried report read as `[absent on this host]`.
    pub home: Option<PathBuf>,
}

impl Grounds {
    /// The grounds with `report` as the first prior report, for a launch that climbs from it; the ledger may not
    /// hold it yet.
    pub fn carrying(mut self, report: Option<&str>) -> Self {
        if let Some(report) = report {
            self.prior.retain(|prior| prior.trim() != report);
            self.prior.insert(0, report.to_owned());
            self.prior.truncate(PRIOR_REPORTS);
        }
        self
    }

    /// The ETA line's value: the tune cell, else the ledger's p75 with its n, either scaled by the
    /// tier's calibration, else what the issue declares, else the uncalibrated default.
    pub fn eta(&self, declared: Option<&str>, route: &Route) -> String {
        let aside = declared.map_or_else(String::new, |declared| format!("; declared {declared}"));
        if let Some(constant) = &self.set_aside.eta {
            return format!("{:.0} min (the p75 of every worker run, ledger 7d; {}{aside})", constant.value, constant.why);
        }
        let (minutes, source) = match (&self.eta_cell, &self.figures) {
            (Some(tuned), _) => (tuned.value.ceil(), format!("tune cell, {}", tuned.date)),
            (None, Some(figures)) => (eta_minutes(figures), format!("p75 of n={} runs, {}", figures.runs, self.cell(route))),
            (None, None) => return declared.map_or_else(|| UNCALIBRATED.to_owned(), str::to_owned),
        };
        let scale = self.eta_scale.as_ref().map_or_else(String::new, |scale| format!("; {minutes:.0} min {}", scale.label));
        format!("{:.0} min ({source}{scale}{aside})", self.scaled(minutes))
    }

    /// The minutes the ETA line gives the box when the tune cell or the ledger measured it, scaled
    /// by the tier's calibration.
    pub fn minutes(&self) -> Option<f64> {
        if let Some(constant) = &self.set_aside.eta {
            return Some(constant.value);
        }
        self.measured_minutes().map(|measured| self.scaled(measured))
    }

    fn scaled(&self, measured: f64) -> f64 {
        scaled(measured, self.eta_scale.as_ref().map(|scale| scale.factor), true)
    }

    fn measured_minutes(&self) -> Option<f64> {
        self.eta_cell.as_ref().map(|tuned| tuned.value.ceil()).or_else(|| self.figures.as_ref().map(eta_minutes))
    }

    fn cell(&self, route: &Route) -> String {
        format!("cell {} on {}, ledger {WINDOW}", route.category, self.family)
    }

    pub fn measured(&self) -> bool {
        self.figures.is_some() || self.eta_cell.is_some() || self.budget_cell.is_some() || self.set_aside.eta.is_some() || self.set_aside.budget.is_some()
    }

    pub fn budget(&self, route: &Route) -> Option<String> {
        let (p50, p90, source) = match (&self.set_aside.budget, &self.budget_cell, &self.figures) {
            (Some(constant), _, figures) => (figures.as_ref().map(|figures| figures.median_usd), constant.value, format!("the p90 of every worker run, ledger 7d; {}", constant.why)),
            (None, Some(tuned), figures) => (figures.as_ref().map(|figures| figures.median_usd), tuned.value, format!("tune cell, {}", tuned.date)),
            (None, None, Some(figures)) => (Some(figures.median_usd), figures.p90_usd, format!("p50 and p90 of n={} closed boxes, {}", figures.closed, self.cell(route))),
            (None, None, None) => return None,
        };
        let (p50, source) = match &self.set_aside.cost {
            Some(constant) => (Some(constant.value), format!("{source}; p50 the median of every worker run, {}", constant.why)),
            None => (p50, source),
        };
        let floored = floored_p90(p90, self.floor.map(|(floor, _)| floor));
        let (p90, source) = match self.floor.filter(|_| floored > p90) {
            Some((_, boxes)) => (floored, format!("{source}; the p90 ${p90:.2} is below the median cost of a closed box across every cell (n={boxes} boxes, ledger {WINDOW}), which one real box needs")),
            None => (p90, source),
        };
        let stated = match p50.map(|p50| p50.min(p90)) {
            Some(p50) => format!("E p50 ${p50:.2}, p90 ${p90:.2}"),
            None => format!("E p90 ${p90:.2}"),
        };
        Some(format!("Budget: {stated} ({source}); {READ_SPEND}; at ${p90:.2} API-equivalent spend, {AT_THE_LINE}"))
    }

    /// The issue's Not required lines and the prior reports on it.
    pub fn trace(&self) -> Vec<String> {
        let mut lines: Vec<String> = self.not_required.iter().map(|line| format!("Not required: {line}")).collect();
        lines.extend(self.prior.iter().map(|report| format!("Prior report: {}", carried(report, self.home.as_deref()))));
        lines
    }

    pub fn rationale(&self, route: &Route) -> String {
        let tuned = self.eta_cell.as_ref().or(self.budget_cell.as_ref()).map(|tuned| format!("north tune's cells of {}", tuned.date));
        let live = self.figures.is_some().then(|| format!("the ledger's closed boxes of the last {WINDOW}"));
        let source = match (tuned, live) {
            (Some(tuned), Some(live)) if self.eta_cell.is_none() || self.budget_cell.is_none() => format!("{tuned}, else {live}"),
            (Some(tuned), _) => tuned,
            (None, Some(live)) => live,
            (None, None) => "unmeasured for this cell".to_owned(),
        };
        format!("Why: {}; the ETA and budget are {source}", route.why)
    }
}

/// The carried report as the brief says it: each path under this host's home that this host no longer holds reads
/// [`ABSENT`], keeping whitespace and the word's own punctuation, so a worker is not sent to a lane that is gone; the
/// whole is cut to [`REPORT_CHARS`].
fn carried(report: &str, home: Option<&Path>) -> String {
    let Some(home) = home else { return report.chars().take(REPORT_CHARS).collect() };
    let mut out = String::with_capacity(report.len());
    for part in report.split_inclusive(char::is_whitespace) {
        let word = part.trim_end_matches(char::is_whitespace);
        let core = word.trim_matches(|c: char| BRACKETS.contains(c));
        let path = crate::platform::home_paths::expand(core, home);
        if !core.is_empty() && path != home && path.starts_with(home) && !path.exists() {
            let lead = word.len() - word.trim_start_matches(|c: char| BRACKETS.contains(c)).len();
            out.push_str(&word[..lead]);
            out.push_str(ABSENT);
            out.push_str(&word[lead + core.len()..]);
        } else {
            out.push_str(word);
        }
        out.push_str(&part[word.len()..]);
    }
    out.chars().take(REPORT_CHARS).collect()
}

/// The Budget's p90 raised to `floor`, the median cost of a closed box across every cell, when it sits under it.
pub fn floored_p90(p90: f64, floor: Option<f64>) -> f64 {
    floor.filter(|floor| p90 < *floor).unwrap_or(p90)
}

/// The minutes a brief's ETA gives a box in a cell with these figures.
pub fn eta_minutes(figures: &Figures) -> f64 {
    figures.p75_minutes.ceil()
}

/// The issue's Not required lines: those under a `Not required` heading, and `Not required: TEXT` lines.
pub fn not_required(body: &str) -> Vec<String> {
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
