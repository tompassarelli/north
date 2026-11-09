use north_base::platform::harness;

use crate::ledger::closed::ClosedReport;
use crate::router::settle::{self, Arm, Outcome};

pub const TABLE: &str = "harness/routing-cells.toml";
const EFFORT_WORDS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
pub const LIVE: usize = 3;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Cell {
    pub model: &'static str,
    pub effort: &'static str,
}

impl Cell {
    pub fn name(self) -> String {
        format!("{}-{}", self.model, self.effort)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Leader,
    Challenger,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Self::Leader => "leader",
            Self::Challenger => "challenger",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Live {
    pub cell: Cell,
    pub role: Role,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Exclusion {
    pub model: Option<&'static str>,
    pub effort: Option<&'static str>,
    pub categories: Vec<&'static str>,
    pub reason: &'static str,
}

impl Exclusion {
    fn matches(&self, category: &str, model: &str, effort: &str) -> bool {
        self.model.is_none_or(|named| named == model)
            && self.effort.is_none_or(|named| named == effort)
            && (self.categories.is_empty() || self.categories.contains(&category))
    }
}

fn strings(key: &str) -> Vec<&'static str> {
    harness::document(TABLE).get(key).and_then(toml_edit::Item::as_array).map(|array| array.iter().filter_map(toml_edit::Value::as_str).collect()).unwrap_or_default()
}

pub fn models() -> Vec<&'static str> {
    strings("models")
}

pub fn efforts() -> Vec<&'static str> {
    strings("efforts")
}

pub fn exclusions() -> Vec<Exclusion> {
    let document = harness::document(TABLE);
    let Some(tables) = document.get("exclude").and_then(toml_edit::Item::as_array_of_tables) else { return Vec::new() };
    tables
        .iter()
        .map(|table| Exclusion {
            model: table.get("model").and_then(toml_edit::Item::as_str),
            effort: table.get("effort").and_then(toml_edit::Item::as_str),
            categories: table.get("categories").and_then(toml_edit::Item::as_array).map(|array| array.iter().filter_map(toml_edit::Value::as_str).collect()).unwrap_or_default(),
            reason: table.get("reason").and_then(toml_edit::Item::as_str).unwrap_or_default(),
        })
        .collect()
}

pub fn allowed(category: &str) -> Vec<Cell> {
    let exclusions = exclusions();
    models()
        .into_iter()
        .flat_map(|model| efforts().into_iter().map(move |effort| Cell { model, effort }))
        .filter(|cell| !exclusions.iter().any(|row| row.matches(category, cell.model, cell.effort)))
        .collect()
}

pub fn excluded(category: &str, model: &str, effort: &str) -> Option<&'static str> {
    let model = family(model).unwrap_or(model);
    exclusions().into_iter().find(|row| row.matches(category, model, effort)).map(|row| row.reason)
}

pub fn family(model: &str) -> Option<&'static str> {
    let model = model.to_lowercase();
    models().into_iter().find(|family| model.contains(family))
}

pub fn table_line() -> String {
    let cells: Vec<String> = allowed("").into_iter().map(Cell::name).collect();
    format!("arm cells ({}): {}", TABLE, cells.join(", "))
}

pub fn named(model: &str, effort: &str) -> Option<String> {
    let model = family(model)?;
    let effort = EFFORT_WORDS.into_iter().find(|word| *word == effort)?;
    Some(format!("{model}-{effort}"))
}

pub fn outcomes(report: &ClosedReport, category: &str) -> std::collections::BTreeMap<String, Vec<Outcome>> {
    let mut outcomes: std::collections::BTreeMap<String, Vec<Outcome>> = std::collections::BTreeMap::new();
    for armed in report.cell_boxes.iter().filter(|armed| armed.category == category) {
        outcomes.entry(armed.cell.clone()).or_default().push(Outcome { usd: armed.usd, closed: armed.closed, kept: armed.closed && !armed.reworked });
    }
    outcomes
}

fn boxes(outcomes: &std::collections::BTreeMap<String, Vec<Outcome>>, cell: Cell) -> &[Outcome] {
    outcomes.get(&cell.name()).map_or(&[], Vec::as_slice)
}

fn cost(outcomes: &std::collections::BTreeMap<String, Vec<Outcome>>, cell: Cell) -> Option<f64> {
    settle::usd_per_kept(boxes(outcomes, cell))
}

fn beats(outcomes: &std::collections::BTreeMap<String, Vec<Outcome>>, first: Cell, second: Cell) -> bool {
    let arm = |cell: Cell| Arm { runs: boxes(outcomes, cell).len(), boxes: boxes(outcomes, cell) };
    settle::settled(arm(first), arm(second)) == Some(settle::Side::First)
}

fn distance(first: Cell, second: Cell) -> usize {
    let model = |cell: Cell| models().iter().position(|model| *model == cell.model).unwrap_or(0);
    let effort = |cell: Cell| efforts().iter().position(|effort| *effort == cell.effort).unwrap_or(0);
    model(first).abs_diff(model(second)) + effort(first).abs_diff(effort(second))
}

pub fn leader(category: &str, seed: Cell, outcomes: &std::collections::BTreeMap<String, Vec<Outcome>>) -> Cell {
    let allowed = allowed(category);
    let priced: Vec<Cell> = allowed.iter().copied().filter(|cell| !boxes(outcomes, *cell).is_empty()).collect();
    let cheapest = |cells: &[Cell]| cells.iter().copied().min_by(|a, b| cost(outcomes, *a).unwrap_or(f64::INFINITY).total_cmp(&cost(outcomes, *b).unwrap_or(f64::INFINITY)).then(a.cmp(b)));
    let mut leader = if priced.contains(&seed) { seed } else { cheapest(&priced).unwrap_or(seed) };
    for cell in priced {
        if cell != leader && beats(outcomes, cell, leader) {
            leader = cell;
        }
    }
    if allowed.contains(&leader) { leader } else { allowed[0] }
}

pub fn live(category: &str, seed: Cell, outcomes: &std::collections::BTreeMap<String, Vec<Outcome>>) -> Vec<Live> {
    let allowed = allowed(category);
    if allowed.is_empty() {
        return Vec::new();
    }
    let leader = leader(category, seed, outcomes);
    let mut candidates: Vec<Cell> = allowed.iter().copied().filter(|cell| *cell != leader).collect();
    candidates.sort_by(|a, b| distance(*a, leader).cmp(&distance(*b, leader)).then(a.cmp(b)));
    let mut live = vec![Live { cell: leader, role: Role::Leader }];
    for cell in candidates {
        if live.len() >= LIVE {
            break;
        }
        if beats(outcomes, leader, cell) {
            continue;
        }
        live.push(Live { cell, role: Role::Challenger });
    }
    live
}

pub fn shares(live: &[Live], outcomes: &std::collections::BTreeMap<String, Vec<Outcome>>, category: &str) -> (Vec<f64>, bool) {
    let arms: Vec<&[Outcome]> = live.iter().map(|live| boxes(outcomes, live.cell)).collect();
    settle::shares(&arms, &format!("{category} cell"))
}

pub fn draw(category: &str, seed: Cell, seed_text: &str, outcomes: &std::collections::BTreeMap<String, Vec<Outcome>>) -> (Cell, Role, String) {
    let live = live(category, seed, outcomes);
    if let [only] = live.as_slice() {
        return (only.cell, only.role, format!("draws {} as its leader, the only live cell of {}", only.cell.name(), allowed(category).len()));
    }
    let (weights, settled) = shares(&live, outcomes, category);
    let at = settle::pick(&weights, seed_text);
    let drawn = live[at];
    let listed: Vec<String> = live.iter().zip(&weights).map(|(live, share)| format!("{} ({}) {:.0}%", live.cell.name(), live.role.name(), 100.0 * share)).collect();
    let settled = if settled { ", settled" } else { "" };
    (drawn.cell, drawn.role, format!("draws {} as a {} on its {:.0}% share of the live cells ({}{settled}) seeded {seed_text}", drawn.cell.name(), drawn.role.name(), 100.0 * weights[at], listed.join(", ")))
}
