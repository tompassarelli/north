//! The one table `north infra` prints: each live paid resource with the price its source reports, merged by name so a
//! machine the cloud API and the manifest both name appears once, and one total per currency because no source prices
//! in another's currency.
use serde::Serialize;

use super::manifest::Billing;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Currency {
    Eur,
    Usd,
}

impl Currency {
    fn symbol(self) -> &'static str {
        match self {
            Self::Eur => "€",
            Self::Usd => "$",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Price {
    pub amount: f64,
    pub currency: Currency,
}

impl Price {
    pub fn eur(amount: f64) -> Self {
        Self {
            amount,
            currency: Currency::Eur,
        }
    }

    pub fn usd(amount: f64) -> Self {
        Self {
            amount,
            currency: Currency::Usd,
        }
    }

    /// The amount with its currency's mark, to two places, as every source reports it gross.
    pub fn line(self) -> String {
        format!("{}{:.2}", self.currency.symbol(), self.amount)
    }
}

/// What the capacity ledger says a machine served in the window: the fleet sessions and build jobs it ran, and the
/// share of a core each of its cores carried. `None` until the ledger is read, so an unread ledger never marks a busy
/// machine idle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Usage {
    pub sessions: usize,
    pub jobs: usize,
    pub load_per_core: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Resource {
    pub provider: String,
    pub name: String,
    pub kind: String,
    pub location: Option<String>,
    pub status: String,
    pub price: Option<Price>,
    /// The machine's CPU count, from south's manifest, which turns the ledger's CPU seconds into load per core.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cores: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

impl Resource {
    pub fn new(provider: &str, name: impl Into<String>, kind: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            name: name.into(),
            kind: kind.to_owned(),
            location: None,
            status: String::new(),
            price: None,
            cores: None,
            usage: None,
        }
    }

    pub fn is_machine(&self) -> bool {
        matches!(self.kind.as_str(), "machine" | "server")
    }

    /// True when the capacity ledger served this machine no job in the window, so a paid box that has done nothing
    /// shows as idle rather than as a cost with no sign of use.
    pub fn idle(&self) -> bool {
        self.usage.is_some_and(|usage| usage.jobs == 0)
    }

    pub fn at(mut self, location: Option<String>) -> Self {
        self.location = location;
        self
    }

    pub fn status(mut self, status: impl Into<String>) -> Self {
        self.status = status.into();
        self
    }

    pub fn priced(mut self, price: Option<Price>) -> Self {
        self.price = price;
        self
    }

    pub fn cores(mut self, cores: Option<f64>) -> Self {
        self.cores = cores;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Total {
    pub currency: Currency,
    pub monthly: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub at: String,
    pub resources: Vec<Resource>,
    pub totals: Vec<Total>,
    /// Each source that could not be read, with why; the other sources still print.
    pub unread: Vec<String>,
}

impl Report {
    pub fn of(at: String, resources: Vec<Resource>, unread: Vec<String>) -> Self {
        let totals = totals(&resources);
        Self {
            at,
            resources,
            totals,
            unread,
        }
    }

    /// The total as one phrase, in the order the currencies are listed, `none` when no source priced anything.
    pub fn total_line(&self) -> String {
        if self.totals.is_empty() {
            return "no priced resource".to_owned();
        }
        self.totals
            .iter()
            .map(|total| {
                Price {
                    amount: total.monthly,
                    currency: total.currency,
                }
                .line()
            })
            .collect::<Vec<_>>()
            .join(" + ")
    }

    pub fn text(&self) -> String {
        let mut lines = vec![
            format!(
                "Live paid resources at {} (cloud APIs through the broker, south's infra manifest on disk, work from the capacity ledger over 24 h)",
                self.at
            ),
            String::new(),
        ];
        let header = [
            "provider",
            "resource",
            "kind",
            "location",
            "status",
            "per month",
            "load/core",
            "sessions",
            "jobs",
        ]
        .map(str::to_owned);
        let mut rows: Vec<[String; 9]> = self
            .resources
            .iter()
            .map(|resource| {
                let (load, sessions, jobs) = workload_cells(resource);
                [
                    resource.provider.clone(),
                    resource.name.clone(),
                    resource.kind.clone(),
                    resource.location.clone().unwrap_or_else(|| "-".to_owned()),
                    if resource.status.is_empty() {
                        "-".to_owned()
                    } else {
                        resource.status.clone()
                    },
                    resource.price.map_or_else(|| "-".to_owned(), Price::line),
                    load,
                    sessions,
                    jobs,
                ]
            })
            .collect();
        rows.insert(0, header);
        lines.extend(grid(&rows));
        lines.push(String::new());
        lines.push(format!("total: {} a month", self.total_line()));
        for unread in &self.unread {
            lines.push(format!("unread: {unread}"));
        }
        lines.join("\n")
    }
}

/// Each priced resource's monthly amount summed per currency, never across currencies.
pub fn totals(resources: &[Resource]) -> Vec<Total> {
    let mut found: Vec<Total> = Vec::new();
    for price in resources.iter().filter_map(|resource| resource.price) {
        match found
            .iter_mut()
            .find(|total| total.currency == price.currency)
        {
            Some(total) => total.monthly += price.amount,
            None => found.push(Total {
                currency: price.currency,
                monthly: price.amount,
            }),
        }
    }
    found.sort_by_key(|total| total.currency);
    found
}

/// The load, sessions and jobs columns: `idle` when the ledger served the machine no job, `-` for a service or an
/// unread ledger, and the figure otherwise.
fn workload_cells(resource: &Resource) -> (String, String, String) {
    if !resource.is_machine() {
        return ("-".to_owned(), "-".to_owned(), "-".to_owned());
    }
    let Some(usage) = resource.usage else {
        return ("-".to_owned(), "-".to_owned(), "-".to_owned());
    };
    if resource.idle() {
        return ("idle".to_owned(), "0".to_owned(), "0".to_owned());
    }
    (
        usage
            .load_per_core
            .map_or_else(|| "-".to_owned(), |load| format!("{load:.2}")),
        usage.sessions.to_string(),
        usage.jobs.to_string(),
    )
}

/// Fills each machine's workload from the ledger window, priced against the cores the manifest names: CPU seconds over
/// the window and those cores is the share of a core it carried. No ledger leaves every machine unread, not idle.
pub fn workloads(
    resources: &mut [Resource],
    ledger: Option<&std::collections::BTreeMap<String, crate::cli::infra::capacity::Record>>,
) {
    let Some(ledger) = ledger else { return };
    for resource in resources
        .iter_mut()
        .filter(|resource| resource.is_machine())
    {
        let record = ledger.get(&resource.name).copied().unwrap_or_default();
        let load_per_core = resource.cores.filter(|cores| *cores > 0.0).map(|cores| {
            record.cpu_seconds / (crate::cli::infra::capacity::WINDOW_SECONDS as f64 * cores)
        });
        resource.usage = Some(Usage {
            sessions: record.sessions,
            jobs: record.jobs,
            load_per_core,
        });
    }
}

/// Live provider rows first, then the manifest's paid machines and services whose name no live source already holds, so
/// cpx62 is one row read live rather than two. A machine the manifest names carries its CPU count whichever source
/// supplied the row.
pub fn merge(live: Vec<Resource>, billing: &Billing) -> Vec<Resource> {
    let mut resources: Vec<Resource> = live
        .into_iter()
        .map(|resource| {
            let cores = billing
                .machines
                .iter()
                .find(|machine| machine.id == resource.name)
                .and_then(|machine| machine.cores);
            resource.cores(cores)
        })
        .collect();
    for machine in billing
        .machines
        .iter()
        .filter(|machine| machine.monthly_usd > 0.0)
    {
        if resources.iter().all(|resource| resource.name != machine.id) {
            resources.push(
                Resource::new("manifest", machine.id.clone(), "machine")
                    .cores(machine.cores)
                    .priced(Some(Price::usd(machine.monthly_usd))),
            );
        }
    }
    for service in billing.paid_services() {
        if resources.iter().all(|resource| resource.name != service.id) {
            let status = service
                .plan
                .clone()
                .unwrap_or_else(|| "manifest".to_owned());
            resources.push(
                Resource::new("manifest", service.id.clone(), "service")
                    .status(status)
                    .priced(service.monthly_usd.map(Price::usd)),
            );
        }
    }
    resources
        .sort_by(|left, right| (&left.provider, &left.name).cmp(&(&right.provider, &right.name)));
    resources
}

fn grid(rows: &[[String; 9]]) -> Vec<String> {
    let widths: Vec<usize> = (0..9)
        .map(|column| {
            rows.iter()
                .map(|row| row[column].chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    let right = [5, 6, 7, 8];
    rows.iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(column, cell)| {
                    if right.contains(&column) {
                        format!("{cell:>width$}", width = widths[column])
                    } else {
                        format!("{cell:<width$}", width = widths[column])
                    }
                })
                .collect::<Vec<_>>()
                .join("  ")
                .trim_end()
                .to_owned()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::infra::manifest;

    const MANIFEST: &str = r#"
[[machine]]
id = "cpx62"
monthly_usd = 171.71
hourly_eur = 0.2452
monthly_cap_eur = 152.99
plan = "CPX62 server (16 shared vCPU, 32 GB)"

[[machine]]
id = "gex45"
monthly_usd = 59.0
plan = "GEX45 dedicated, 20 cores, 64 GB"

[[service]]
id = "offfleet-backup"
plan = "Storage Box BX11"
monthly_usd = 12.0

[[service]]
id = "anthropic-a"
plan = "Ultra"
monthly_usd = 216.67
"#;

    fn billing() -> Billing {
        manifest::parse(MANIFEST).expect("the manifest parses")
    }

    #[test]
    fn a_machine_both_sources_name_is_one_row_read_live_and_the_manifest_adds_its_own() {
        let live = vec![
            Resource::new("hetzner", "cpx62", "server")
                .status("running")
                .at(Some("hel1".to_owned()))
                .priced(Some(Price::eur(152.99))),
        ];
        let resources = merge(live, &billing());
        assert_eq!(resources.len(), 3);
        let names: Vec<&str> = resources
            .iter()
            .map(|resource| resource.name.as_str())
            .collect();
        assert_eq!(names, ["cpx62", "gex45", "offfleet-backup"]);
        assert_eq!(resources[0].provider, "hetzner");
        assert_eq!(resources[0].price, Some(Price::eur(152.99)));
        assert_eq!(resources[2].kind, "service");
    }

    #[test]
    fn totals_stay_one_per_currency_and_the_table_names_each_resource_with_its_price() {
        let live = vec![
            Resource::new("hetzner", "cpx62", "server")
                .status("running")
                .priced(Some(Price::eur(152.99))),
        ];
        let mut resources = merge(live, &billing());
        let ledger = std::collections::BTreeMap::from([(
            "cpx62".to_owned(),
            crate::cli::infra::capacity::Record {
                sessions: 3,
                jobs: 9,
                cpu_seconds: crate::cli::infra::capacity::WINDOW_SECONDS as f64 * 16.0 * 0.25,
            },
        )]);
        workloads(&mut resources, Some(&ledger));
        assert!(
            !resources
                .iter()
                .find(|resource| resource.name == "cpx62")
                .is_some_and(Resource::idle),
            "a machine with jobs is not idle"
        );
        assert!(
            resources
                .iter()
                .find(|resource| resource.name == "gex45")
                .is_some_and(Resource::idle),
            "a machine with no job in 24 h is idle"
        );
        let report = Report::of("2026-11-10T10:40:00Z".to_owned(), resources, Vec::new());
        assert_eq!(
            report.totals,
            vec![
                Total {
                    currency: Currency::Eur,
                    monthly: 152.99
                },
                Total {
                    currency: Currency::Usd,
                    monthly: 71.0
                }
            ]
        );
        assert_eq!(report.total_line(), "€152.99 + $71.00");
        let text = report.text();
        for named in ["cpx62", "gex45", "offfleet-backup"] {
            assert!(text.contains(named), "{text}");
        }
        assert!(text.contains("€152.99"), "{text}");
        assert!(text.contains("$71.00 a month"), "{text}");
        assert!(text.contains("load/core"), "{text}");
        assert!(text.contains("0.25"), "{text}");
        assert!(text.contains("idle"), "{text}");
    }

    #[test]
    fn a_source_that_cannot_be_read_leaves_the_other_rows_standing() {
        let report = Report::of(
            "2026-11-10T10:40:00Z".to_owned(),
            merge(Vec::new(), &billing()),
            vec!["hcloud: the broker refused".to_owned()],
        );
        let text = report.text();
        assert!(text.contains("gex45"), "{text}");
        assert!(
            text.contains("unread: hcloud: the broker refused"),
            "{text}"
        );
    }
}
