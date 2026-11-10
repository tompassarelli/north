//! What the fleet costs by the month, read from south's infra manifest on disk: each Claude login's plan and price,
//! each machine's monthly cost, and every other service with a monthly figure. Nothing is fetched from a provider.
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
pub struct Service {
    pub id: String,
    pub plan: Option<String>,
    pub monthly_usd: Option<f64>,
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
    /// Every manifest service, including the Claude logins `plans` names by account.
    pub services: Vec<Service>,
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

    /// The services that are neither Claude logins nor free, so `north infra` lists only paid infrastructure.
    pub fn paid_services(&self) -> impl Iterator<Item = &Service> {
        self.services.iter().filter(|service| !service.id.starts_with(LOGIN_PREFIX) && service.monthly_usd.is_some_and(|usd| usd > 0.0))
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

fn tables<'a>(document: &'a DocumentMut, key: &str) -> Vec<&'a Table> {
    document.get(key).and_then(Item::as_array_of_tables).map(|tables| tables.iter().collect()).unwrap_or_default()
}

fn text(table: &Table, key: &str) -> Option<String> {
    table.get(key).and_then(Item::as_str).map(str::to_owned)
}

fn number(table: &Table, key: &str) -> Option<f64> {
    table.get(key).and_then(|item| item.as_float().or_else(|| item.as_integer().map(|whole| whole as f64)))
}

pub(crate) fn parse(source: &str) -> Result<Billing, String> {
    let document: DocumentMut = source.parse().map_err(|error| format!("{error}"))?;
    let services: Vec<Service> = tables(&document, "service")
        .into_iter()
        .filter_map(|table| {
            Some(Service { id: text(table, "id")?, plan: text(table, "plan"), monthly_usd: number(table, "monthly_usd") })
        })
        .collect();
    let plans = services
        .iter()
        .filter_map(|service| {
            let account = service.id.strip_prefix(LOGIN_PREFIX)?.to_owned();
            Some((account, Plan { plan: service.plan.clone()?, monthly_usd: service.monthly_usd? }))
        })
        .collect();
    let machines = tables(&document, "machine")
        .into_iter()
        .filter_map(|table| {
            Some(Machine { id: text(table, "id")?, monthly_usd: number(table, "monthly_usd")?, hourly_eur: number(table, "hourly_eur"), monthly_cap_eur: number(table, "monthly_cap_eur") })
        })
        .collect();
    Ok(Billing { plans, services, machines })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"
[[machine]]
id = "whiterabbit"
monthly_usd = 0.0

[[machine]]
id = "nexus"
monthly_usd = 48.0

[[machine]]
id = "cpx62"
hourly_eur = 0.2452
monthly_cap_eur = 152.99
monthly_usd = 171.71

[[service]]
id = "anthropic-a"
plan = "Ultra"
monthly_usd = 216.67

[[service]]
id = "offfleet-backup"
plan = "Storage Box BX11"
monthly_usd = "unread"

[[service]]
id = "github"
plan = "free"
monthly_usd = "unread"
"#;

    #[test]
    fn the_manifest_keeps_each_login_plan_and_paid_service_out_of_the_machines_total() {
        let billing = parse(MANIFEST).expect("the manifest parses");
        assert_eq!(billing.plans["a"].plan, "Ultra");
        assert!((billing.plans["a"].monthly_usd - 216.67).abs() < f64::EPSILON);
        assert_eq!(billing.machines.len(), 3);
        assert!((billing.total_usd() - 219.71).abs() < 0.001);
        assert!(billing.capped());
        assert!(billing.paid_services().next().is_none(), "a service whose price is unread is not listed as paid");
    }

    #[test]
    fn a_paid_service_is_listed_beside_the_machines() {
        let text = r#"
[[machine]]
id = "gex45"
monthly_usd = 59.0

[[service]]
id = "offfleet-backup"
plan = "Storage Box BX11"
monthly_usd = 12.0
"#;
        let billing = parse(text).expect("the manifest parses");
        let services: Vec<&str> = billing.paid_services().map(|service| service.id.as_str()).collect();
        assert_eq!(services, ["offfleet-backup"]);
    }
}
