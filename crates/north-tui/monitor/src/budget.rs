use std::fs;
use std::path::Path;

use toml_edit::{DocumentMut, Item};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Green,
    Amber,
    Red,
    Plain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Above,
    Below,
}

#[derive(Clone, Debug)]
pub struct Budget {
    pub id: String,
    pub label: String,
    pub unit: String,
    pub amber: f64,
    pub red: f64,
    pub direction: Direction,
}

impl Budget {
    pub fn tone(&self, value: f64) -> Tone {
        let past = |threshold: f64| match self.direction {
            Direction::Above => value >= threshold,
            Direction::Below => value <= threshold,
        };
        if past(self.red) {
            Tone::Red
        } else if past(self.amber) {
            Tone::Amber
        } else {
            Tone::Green
        }
    }
}

const DEFAULTS: &[(&str, &str, &str, f64, f64, Direction)] = &[
    ("queue_depth", "landing queue depth", "tickets", 5.0, 10.0, Direction::Above),
    ("queue_growth", "queue growth over 5 min", "tickets", 1.0, 3.0, Direction::Above),
    ("land_p50", "time to land p50", "s", 300.0, 900.0, Direction::Above),
    ("land_p95", "time to land p95", "s", 900.0, 1800.0, Direction::Above),
    ("landed_hour", "landings in the last hour", "landings", 1e9, 1e9, Direction::Above),
    ("failure_rate", "landing failure rate 6h", "%", 20.0, 40.0, Direction::Above),
    ("landers_down", "landers down", "landers", 1.0, 1.0, Direction::Above),
    ("fleet_queued", "sessions queued", "sessions", 3.0, 8.0, Direction::Above),
    ("fleet_busy", "busy sessions", "sessions", 1e9, 1e9, Direction::Above),
    ("fleet_idle", "idle sessions", "sessions", 1e9, 1e9, Direction::Above),
    ("runtime_skew", "hosts off the common runtime", "hosts", 1.0, 2.0, Direction::Above),
    ("stale_processes", "processes on an old runtime", "processes", 1.0, 10.0, Direction::Above),
    ("rollbacks_24h", "runtime rollbacks in 24h", "rollbacks", 1.0, 3.0, Direction::Above),
    ("health_red", "failing health checks", "checks", 1.0, 3.0, Direction::Above),
    ("compile_s", "compile time of the last land check", "s", 120.0, 300.0, Direction::Above),
    ("test_s", "test time of the last land check", "s", 300.0, 600.0, Direction::Above),
    ("land_checks_s", "land check wall time", "s", 600.0, 1200.0, Direction::Above),
    ("load_per_core", "load per core", "load", 1.0, 2.0, Direction::Above),
    ("ladder", "ladder level", "level", 1.0, 2.0, Direction::Above),
];

#[derive(Clone, Debug)]
pub struct Budgets {
    pub entries: Vec<Budget>,
    pub source: String,
}

impl Budgets {
    pub fn defaults() -> Self {
        let entries = DEFAULTS
            .iter()
            .map(|(id, label, unit, amber, red, direction)| Budget { id: (*id).into(), label: (*label).into(), unit: (*unit).into(), amber: *amber, red: *red, direction: *direction })
            .collect();
        Self { entries, source: "built-in defaults".into() }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut budgets = Self::defaults();
        budgets.merge(&text).map_err(|error| format!("{}: {error}", path.display()))?;
        budgets.source = path.display().to_string();
        Ok(budgets)
    }

    pub fn merge(&mut self, text: &str) -> Result<(), String> {
        let document: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| error.to_string())?;
        let Some(tables) = document.get("budget").and_then(Item::as_array_of_tables) else {
            return Ok(());
        };
        for table in tables {
            let text_of = |key: &str| table.get(key).and_then(Item::as_str).map(str::to_owned);
            let number_of = |key: &str| table.get(key).and_then(|item| item.as_float().or_else(|| item.as_integer().map(|whole| whole as f64)));
            let id = text_of("id").ok_or("a [[budget]] has no id")?;
            let direction = match text_of("direction").as_deref() {
                None | Some("above") => Direction::Above,
                Some("below") => Direction::Below,
                Some(other) => return Err(format!("budget {id}: direction {other} is neither above nor below")),
            };
            let amber = number_of("amber").ok_or_else(|| format!("budget {id} has no amber"))?;
            let red = number_of("red").ok_or_else(|| format!("budget {id} has no red"))?;
            let label = text_of("label").unwrap_or_else(|| id.clone());
            let unit = text_of("unit").unwrap_or_default();
            let budget = Budget { id: id.clone(), label, unit, amber, red, direction };
            match self.entries.iter_mut().find(|entry| entry.id == id) {
                Some(entry) => *entry = budget,
                None => self.entries.push(budget),
            }
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Budget> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn tone(&self, id: &str, value: f64) -> Tone {
        self.get(id).map_or(Tone::Plain, |budget| budget.tone(value))
    }
}
