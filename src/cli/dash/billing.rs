//! What the fleet costs by the month, read from south's infra manifest on disk: each Claude login's plan and price and
//! each machine's monthly cost, an hourly host with its rate and its monthly cap. Nothing is fetched from a provider.
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;
use toml_edit::{DocumentMut, Item, Table};

const MANIFEST: &str = "code/south/main/infra/manifest.toml";
const LOGIN_PREFIX: &str = "anthropic-";

#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    pub plan: String,
    pub monthly_usd: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Machine {
    pub id: String,
    pub monthly_usd: f64,
    pub hourly_eur: Option<f64>,
    pub monthly_cap_eur: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Billing {
    pub plans: BTreeMap<String, Plan>,
    pub machines: Vec<Machine>,
}

impl Billing {
    pub fn total_usd(&self) -> f64 {
        self.machines.iter().map(|machine| machine.monthly_usd).sum()
    }

    /// True when a machine bills by the hour up to a cap, so the total is the most a month can cost.
    pub fn capped(&self) -> bool {
        self.machines.iter().any(|machine| machine.hourly_eur.is_some())
    }
}

pub fn path() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(MANIFEST)
}

pub fn read() -> Result<Billing, String> {
    let path = path();
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    parse(&text).map_err(|why| format!("{}: {why}", path.display()))
}

pub(super) fn parse(text: &str) -> Result<Billing, String> {
    let document: DocumentMut = text.parse().map_err(|error| format!("{error}"))?;
    let tables = |key: &str| document.get(key).and_then(Item::as_array_of_tables).map(|tables| tables.iter().collect::<Vec<&Table>>()).unwrap_or_default();
    let text = |table: &Table, key: &str| table.get(key).and_then(Item::as_str).map(str::to_owned);
    let number = |table: &Table, key: &str| table.get(key).and_then(|item| item.as_float().or_else(|| item.as_integer().map(|whole| whole as f64)));
    let plans = tables("service")
        .into_iter()
        .filter_map(|table| {
            let account = text(table, "id")?.strip_prefix(LOGIN_PREFIX)?.to_owned();
            Some((account, Plan { plan: text(table, "plan")?, monthly_usd: number(table, "monthly_usd")? }))
        })
        .collect();
    let machines = tables("machine")
        .into_iter()
        .filter_map(|table| {
            Some(Machine { id: text(table, "id")?, monthly_usd: number(table, "monthly_usd")?, hourly_eur: number(table, "hourly_eur"), monthly_cap_eur: number(table, "monthly_cap_eur") })
        })
        .collect();
    Ok(Billing { plans, machines })
}
