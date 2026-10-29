use crate::foundation::clock::Timestamp;
use toml_edit::{DocumentMut, Item, Table};

pub(crate) const MEASURES: [&str; 4] = ["cloud_launches", "launch_failures", "closed_boxes", "heartbeat_age_minutes"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Direction {
    Below,
    Above,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Expectation {
    pub name: String,
    pub measure: String,
    pub direction: Direction,
    pub threshold: f64,
    pub window: String,
    pub window_seconds: f64,
    pub owner: String,
    pub fix: String,
    pub on: String,
    pub host: Option<String>,
    pub until: Option<f64>,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Roster {
    pub proxy: String,
    pub expectations: Vec<Expectation>,
}

impl Expectation {
    pub(crate) fn breaching(&self, value: f64) -> bool {
        match self.direction {
            Direction::Below => value < self.threshold,
            Direction::Above => value > self.threshold,
        }
    }

    pub(crate) fn describe(&self, value: f64) -> String {
        let (comparison, kept) = match (self.direction, self.breaching(value)) {
            (Direction::Below, true) => ("<", self.threshold),
            (Direction::Below, false) => (">=", self.threshold),
            (Direction::Above, true) => (">", self.threshold),
            (Direction::Above, false) => ("<=", self.threshold),
        };
        format!("{} {value} {comparison} {kept} over {}", self.measure, self.window)
    }
}

pub(crate) fn window_seconds(window: &str) -> Result<f64, String> {
    let split = window.find(|character: char| !character.is_ascii_digit()).ok_or_else(|| format!("window {window:?} has no unit"))?;
    let (count, unit) = window.split_at(split);
    let unit = match unit {
        "m" => 60.0,
        "h" => 3_600.0,
        "d" => 86_400.0,
        _ => return Err(format!("window {window:?} must end in m, h or d")),
    };
    count.parse::<f64>().map(|count| count * unit).map_err(|_| format!("window {window:?} has no count"))
}

fn text(table: &Table, name: &str, key: &str) -> Result<String, String> {
    table.get(key).and_then(Item::as_str).filter(|value| !value.is_empty()).map(str::to_owned).ok_or_else(|| format!("expectation {name}: {key} is missing"))
}

fn expectation(table: &Table) -> Result<Expectation, String> {
    let name = table.get("name").and_then(Item::as_str).filter(|name| !name.is_empty()).ok_or("an expectation has no name")?.to_owned();
    let measure = text(table, &name, "measure")?;
    if !MEASURES.contains(&measure.as_str()) {
        return Err(format!("expectation {name}: measure {measure} is not one of {}", MEASURES.join(", ")));
    }
    let direction = match text(table, &name, "breach_when")?.as_str() {
        "below" => Direction::Below,
        "above" => Direction::Above,
        other => return Err(format!("expectation {name}: breach_when {other} is not below or above")),
    };
    let threshold = table.get("threshold").and_then(|item| item.as_float().or_else(|| item.as_integer().map(|whole| whole as f64))).ok_or_else(|| format!("expectation {name}: threshold is missing"))?;
    let window = text(table, &name, "window")?;
    let on = text(table, &name, "on")?;
    if !["nexus", "elsewhere"].contains(&on.as_str()) {
        return Err(format!("expectation {name}: on {on} is not nexus or elsewhere"));
    }
    let until = match table.get("until").and_then(Item::as_str) {
        None => None,
        Some(date) => Some(Timestamp::parse(&format!("{date}T00:00:00Z")).map_err(|_| format!("expectation {name}: until {date} is not a date"))?.unix_seconds_f64()),
    };
    Ok(Expectation {
        window_seconds: window_seconds(&window).map_err(|why| format!("expectation {name}: {why}"))?,
        owner: text(table, &name, "owner")?,
        fix: text(table, &name, "fix")?,
        host: table.get("host").and_then(Item::as_str).map(str::to_owned),
        name,
        measure,
        direction,
        threshold,
        window,
        on,
        until,
    })
}

pub(crate) fn parse(source: &str) -> Result<Roster, String> {
    let document: DocumentMut = source.parse().map_err(|error| format!("expectations: {error}"))?;
    let proxy = document.get("notify").and_then(|notify| notify.get("proxy")).and_then(Item::as_str).filter(|proxy| !proxy.is_empty()).ok_or("expectations: [notify] proxy is missing")?.to_owned();
    let tables = document.get("expectation").and_then(Item::as_array_of_tables).ok_or("expectations: no [[expectation]] tables")?;
    let expectations = tables.iter().map(expectation).collect::<Result<Vec<_>, _>>()?;
    for (index, one) in expectations.iter().enumerate() {
        if expectations[..index].iter().any(|earlier| earlier.name == one.name) {
            return Err(format!("expectations: {} is listed twice", one.name));
        }
    }
    Ok(Roster { proxy, expectations })
}
