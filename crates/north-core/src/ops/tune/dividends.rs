//! The dividends of landed plan issues: each is a figure the 7-day ledger gives and the bar it must
//! stay inside. A tick reads every one once a day and keeps the reading, so a landed change that
//! stops paying turns DECAYED in the readings and tells the proxy once.
use crate::ledger::ClosedReport;
use serde_json::{Value, json};
use toml_edit::{DocumentMut, Item, Table};

const DEFINITIONS: &str = include_str!("dividends.toml");

#[derive(Clone, Debug, PartialEq)]
pub enum Metric {
    UsdPerClosed,
    LeadsUsdPerClosed,
    FamilyShare { family: String, categories: Vec<String> },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bar {
    AtMost(f64),
    AtLeast(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dividend {
    pub name: String,
    pub issue: String,
    pub metric: Metric,
    pub bar: Bar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Paying,
    Decayed,
    Unread,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Paying => "PAYING",
            Self::Decayed => "DECAYED",
            Self::Unread => "UNREAD",
        }
    }
}

/// One day's reading of a dividend.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub day: String,
    pub name: String,
    pub status: Status,
}

impl Reading {
    pub fn read(row: &Value) -> Option<Self> {
        let status = match row["status"].as_str()? {
            "PAYING" => Status::Paying,
            "DECAYED" => Status::Decayed,
            "UNREAD" => Status::Unread,
            _ => return None,
        };
        Some(Self { day: row["day"].as_str()?.to_owned(), name: row["name"].as_str()?.to_owned(), status })
    }
}

pub fn definitions() -> Result<Vec<Dividend>, String> {
    parse(DEFINITIONS)
}

pub fn parse(text: &str) -> Result<Vec<Dividend>, String> {
    let document: DocumentMut = text.parse().map_err(|error| format!("src/ops/tune/dividends.toml: {error}"))?;
    let tables = document.get("dividend").and_then(Item::as_array_of_tables).ok_or("src/ops/tune/dividends.toml has no [[dividend]]")?;
    tables.iter().map(|table| one(table).map_err(|why| format!("src/ops/tune/dividends.toml: {why}"))).collect()
}

fn one(table: &Table) -> Result<Dividend, String> {
    let text = |key: &str| table.get(key).and_then(Item::as_str).map(str::to_owned).ok_or_else(|| format!("a dividend lacks {key}"));
    let number = |key: &str| table.get(key).and_then(Item::as_value).and_then(|value| value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)));
    let name = text("name")?;
    let metric = match text("metric")?.as_str() {
        "usd_per_closed" => Metric::UsdPerClosed,
        "leads_usd_per_closed" => Metric::LeadsUsdPerClosed,
        "family_share" => Metric::FamilyShare {
            family: text("family")?,
            categories: table
                .get("categories")
                .and_then(Item::as_array)
                .map(|list| list.iter().filter_map(|category| category.as_str().map(str::to_owned)).collect())
                .ok_or_else(|| format!("{name} lacks categories"))?,
        },
        other => return Err(format!("{name}: unknown metric {other}")),
    };
    let bar = match (number("at_most"), number("at_least")) {
        (Some(limit), None) => Bar::AtMost(limit),
        (None, Some(limit)) => Bar::AtLeast(limit),
        _ => return Err(format!("{name} needs exactly one of at_most and at_least")),
    };
    Ok(Dividend { name, issue: text("issue")?, metric, bar })
}

impl Dividend {
    pub fn figure(&self, week: &ClosedReport) -> Option<f64> {
        match &self.metric {
            Metric::UsdPerClosed => week.all_usd_per_closed(),
            Metric::LeadsUsdPerClosed => week.leads_usd_per_closed(),
            Metric::FamilyShare { family, categories } => week.family_share(family, categories),
        }
    }

    fn money(&self) -> bool {
        !matches!(self.metric, Metric::FamilyShare { .. })
    }

    fn show(&self, figure: f64) -> String {
        if self.money() { format!("${figure:.2}") } else { format!("{:.0}%", figure * 100.0) }
    }

    fn bar_text(&self) -> String {
        match self.bar {
            Bar::AtMost(limit) => format!("at most {}", self.show(limit)),
            Bar::AtLeast(limit) => format!("at least {}", self.show(limit)),
        }
    }

    fn status(&self, figure: Option<f64>) -> Status {
        match (figure, self.bar) {
            (None, _) => Status::Unread,
            (Some(figure), Bar::AtMost(limit)) if figure > limit => Status::Decayed,
            (Some(figure), Bar::AtLeast(limit)) if figure < limit => Status::Decayed,
            _ => Status::Paying,
        }
    }
}

/// What a tick adds from the dividends: a line for each, the rows of those with no row yet for
/// today, and the message to the proxy for each that turned DECAYED since its last earlier day.
#[derive(Default)]
pub struct Checked {
    pub lines: Vec<String>,
    pub rows: Vec<(String, Value)>,
    pub alerts: Vec<(String, String)>,
}

pub fn check(week: &ClosedReport, dividends: &[Dividend], earlier: &[Reading], at: &str) -> Checked {
    let day = at.get(..10).unwrap_or(at);
    let mut checked = Checked::default();
    for dividend in dividends {
        let figure = dividend.figure(week);
        let status = dividend.status(figure);
        let shown = figure.map_or_else(|| "no closed box".to_owned(), |figure| dividend.show(figure));
        checked.lines.push(format!("dividend {} {}: {shown} ({}), {}", dividend.issue, dividend.name, dividend.bar_text(), status.label()));
        let rows = || earlier.iter().filter(|row| row.name == dividend.name);
        if rows().any(|row| row.day == day) {
            continue;
        }
        let row = json!({"at": at, "day": day, "name": dividend.name, "issue": dividend.issue, "figure": figure, "bar": dividend.bar_text(), "status": status.label(), "closed": week.closed});
        checked.rows.push((dividend.name.clone(), row));
        let last = rows().filter(|row| row.day.as_str() < day).max_by(|a, b| a.day.cmp(&b.day));
        if status == Status::Decayed && last.is_none_or(|last| last.status != Status::Decayed) {
            let was = last.map_or_else(|| "first reading".to_owned(), |last| format!("{} on {}", last.status.label(), last.day));
            checked.alerts.push((
                dividend.name.clone(),
                format!("dividend DECAYED: {} ({}) reads {shown} over {} closed boxes in the 7d ledger, {} wanted; it was {was}. `north tune` on {at}.", dividend.name, dividend.issue, week.closed, dividend.bar_text()),
            ));
        }
    }
    checked
}
