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

#[derive(Clone, Debug, Serialize)]
pub struct Resource {
    pub provider: String,
    pub name: String,
    pub kind: String,
    pub location: Option<String>,
    pub status: String,
    pub price: Option<Price>,
    /// The machine's CPU count, from south's manifest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cores: Option<f64>,
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
        }
    }

    pub fn is_machine(&self) -> bool {
        matches!(self.kind.as_str(), "machine" | "server")
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
                "Live paid resources at {} (cloud APIs through the broker, south's infra manifest on disk)",
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
        ]
        .map(str::to_owned);
        let mut rows: Vec<[String; 6]> = self
            .resources
            .iter()
            .map(|resource| {
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

fn grid(rows: &[[String; 6]]) -> Vec<String> {
    let widths: Vec<usize> = (0..6)
        .map(|column| {
            rows.iter()
                .map(|row| row[column].chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    let right = [5];
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
