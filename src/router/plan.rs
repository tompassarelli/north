use super::ctx::{Ctx, Metrics, Tier, provider};
use super::moment::{Moment, python_str};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const ASTRA_KEY: &str = "codex:astra";
pub const ASTRA_MODEL: &str = "gpt-6-astra";
pub const ASTRA_EFFORT: &str = "medium";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Capacity {
    pub since: String,
    pub model: Option<String>,
    pub left: Option<f64>,
    pub fallback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Priced {
    #[serde(flatten)]
    pub metrics: Metrics,
    pub price: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Placed {
    pub tier: Option<String>,
    pub account: Option<String>,
    pub explore: bool,
    pub wait: Option<String>,
    pub fast: bool,
    pub fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plan {
    pub accounts: BTreeMap<String, Priced>,
    pub boxes: BTreeMap<String, Placed>,
    pub explore: Vec<String>,
    pub held: Vec<String>,
    pub needs: Vec<String>,
    pub routes: BTreeMap<String, Option<String>>,
    pub warnings: Vec<String>,
    pub capacity: BTreeMap<String, Capacity>,
}

fn price_value(price: f64) -> Value {
    if price == f64::INFINITY {
        json!("unavailable")
    } else {
        json!(price as i64)
    }
}

fn push_new(list: &mut Vec<String>, line: Option<String>) {
    if let Some(line) = line
        && !list.contains(&line)
    {
        list.push(line);
    }
}

impl Ctx {
    pub fn capacity(&self) -> BTreeMap<String, Capacity> {
        let mut out = BTreeMap::new();
        for (p, hit) in &self.capacity {
            let Some(since) = Moment::parse(&hit.since) else {
                continue;
            };
            if self.now.seconds_since(since) > self.knobs.capacity_min * 60.0 {
                continue;
            }
            let name = self.provider_account(p);
            let left = self.accounts.get(&name).and_then(|acct| {
                let m = self.metrics(acct);
                m.projected_left.or(m.used_pct.map(|used| 100.0 - used))
            });
            let floor = self.knobs.codex_floor.as_f64().unwrap_or(f64::INFINITY);
            let astra = p == "codex"
                && hit.model.as_deref() != Some(ASTRA_MODEL)
                && left.is_some_and(|left| left - self.knobs.astra_cost >= floor);
            out.insert(
                p.clone(),
                Capacity {
                    since: hit.since.clone(),
                    model: hit.model.clone(),
                    left,
                    fallback: astra.then(|| ASTRA_KEY.to_owned()),
                },
            );
        }
        out
    }

    pub fn capacity_lines(&self, cap: &BTreeMap<String, Capacity>) -> Vec<String> {
        cap.iter()
            .map(|(p, c)| {
                let at = Moment::parse(&c.since).map_or_else(String::new, Moment::local_clock);
                let model = c.model.as_deref().unwrap_or("None");
                match (&c.fallback, c.left) {
                    (Some(_), Some(left)) => format!(
                        "account {p}: {model} at capacity since {at}; its tiers fall back to {ASTRA_MODEL} {ASTRA_EFFORT} ({left:.1}% left at Fri 23:59 ET, floor {}%)",
                        self.knobs.codex_floor
                    ),
                    _ => format!("account {p}: unavailable (provider capacity, since {at})"),
                }
            })
            .collect()
    }

    pub fn projection_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (name, acct) in self
            .accounts
            .iter()
            .filter(|(_, a)| a.provider.as_deref() == Some("codex"))
        {
            let m = self.metrics(acct);
            let band = acct
                .band
                .clone()
                .unwrap_or_else(|| self.knobs.pace_band.clone());
            let edge = |i: usize| band.get(i).map_or_else(String::new, python_str);
            let left = m.projected_left.map_or_else(
                || "unknown (need two readings in last 3h)".to_owned(),
                |p| format!("{p:.1}%"),
            );
            out.push(format!(
                "{name}: projected {left} left at Fri 23:59 ET (band {}-{})",
                edge(0),
                edge(1)
            ));
            if m.drain_behind {
                out.push("FLAG codex drain behind".into());
            }
        }
        out
    }

    fn with_capacity(&self, cap: &BTreeMap<String, Capacity>) -> Ctx {
        let hit = |t: &Tier| {
            cap.get(provider(&t.key))
                .filter(|c| c.model.is_none() || c.model == t.model)
                .map(|c| c.fallback.is_some())
        };
        let tiers = self.tiers.iter().filter_map(|t| match hit(t) {
            None => Some(t.clone()),
            Some(true) => Some(Tier {
                model: Some(ASTRA_MODEL.into()),
                effort: Some(ASTRA_EFFORT.into()),
                fallback: true,
                ..t.clone()
            }),
            Some(false) => None,
        });
        let tiers: Vec<Tier> = tiers.collect();
        let tiers = tiers
            .into_iter()
            .map(|t| {
                let p = provider(&t.key).to_owned();
                let account = t
                    .account
                    .clone()
                    .or_else(|| self.route(&p))
                    .unwrap_or_else(|| self.provider_account(&p));
                Tier {
                    account: Some(account),
                    ..t
                }
            })
            .collect();
        Ctx {
            tiers,
            ..self.clone()
        }
    }

    fn priced_accounts(&self) -> BTreeMap<String, Priced> {
        self.accounts
            .iter()
            .map(|(n, a)| {
                (
                    n.clone(),
                    Priced {
                        metrics: self.metrics(a),
                        price: price_value(self.price(n)),
                    },
                )
            })
            .collect()
    }

    pub fn plan(&self) -> Result<Plan, String> {
        if let Some((name, acct)) = self.accounts.iter().find(|(_, a)| self.stale(a)) {
            return Err(self.stale_warning(name, acct));
        }
        let cap = self.capacity();
        let ctx = self.with_capacity(&cap);
        let accounts = ctx.priced_accounts();
        let (mut boxes, mut needs, mut warnings) = (BTreeMap::new(), Vec::new(), Vec::new());
        for (slot, item) in ctx.boxes.iter().enumerate() {
            let choice = ctx.choose(item, slot % ctx.knobs.explore_every.max(1) == 0);
            let tier = choice.tier.as_ref();
            let account = tier.map(super::ctx::account_of);
            boxes.insert(
                item.id.clone(),
                Placed {
                    tier: tier.map(|t| t.key.clone()),
                    fast: account.as_deref().is_some_and(|a| ctx.fast(a)),
                    account,
                    explore: choice.explore,
                    wait: choice.wait,
                    fallback: tier.is_some_and(|t| t.fallback),
                },
            );
            push_new(&mut needs, choice.needs);
            push_new(&mut warnings, choice.warn);
        }
        let keys = |keep: &dyn Fn(&Tier) -> bool| {
            ctx.tiers
                .iter()
                .filter(|t| keep(t))
                .map(|t| t.key.clone())
                .collect()
        };
        Ok(Plan {
            accounts,
            boxes,
            explore: keys(&|t| {
                t.runs.unwrap_or(f64::INFINITY) < ctx.knobs.explore_runs && !ctx.successor_soon(t)
            }),
            held: keys(&|t| ctx.successor_soon(t)),
            needs,
            routes: ctx
                .tiers
                .iter()
                .map(|t| (t.key.clone(), ctx.route(provider(&t.key))))
                .collect(),
            warnings,
            capacity: cap,
        })
    }

    fn account_line(&self, name: &str) -> String {
        let m = self.metrics(&self.accounts[name]);
        let pct = |v: Option<f64>| v.map_or_else(|| "unknown".to_owned(), |v| format!("{v:.1}%"));
        format!(
            "account {name}: state {}, used {}, pace {}, available ${:.2}, resets_at {}, fast {}",
            m.state,
            pct(m.used_pct),
            pct(m.pace_pct),
            m.available_usd,
            m.resets_at.as_deref().unwrap_or("unknown"),
            if self.fast(name) { "on" } else { "off" }
        )
    }

    pub fn head_lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self.accounts.keys().map(|n| self.account_line(n)).collect();
        out.extend(self.projection_lines());
        out.extend(self.capacity_lines(&self.capacity()));
        out.extend(
            self.tiers
                .iter()
                .map(|t| format!("route: {}: {}", t.key, self.route_label(provider(&t.key)))),
        );
        out
    }

    pub fn box_lines(&self, result: &Value) -> Vec<String> {
        let mut out = Vec::new();
        for item in &self.boxes {
            let b = &result["boxes"][&item.id];
            let tier = b["tier"].as_str();
            let account = b["account"].as_str().unwrap_or("");
            let wait = b["wait"].as_str().filter(|w| !w.is_empty());
            let what = match (tier.filter(|t| !t.is_empty()), wait) {
                (Some(t), _) if account.contains(':') => format!("{t} on {account}"),
                (Some(t), _) => t.to_owned(),
                (None, Some(w)) => format!("wait for {w}"),
                (None, None) => "none".into(),
            };
            let mut tag = String::new();
            if b["explore"].as_bool() == Some(true) {
                let codex = tier.is_some_and(|t| t.starts_with("codex:"));
                tag.push_str(if codex {
                    " (explore: brief header Explore: codex)"
                } else {
                    " (explore)"
                });
            }
            if b["fallback"].as_bool() == Some(true) {
                tag.push_str(&format!(
                    " on {ASTRA_MODEL} {ASTRA_EFFORT} (capacity fallback)"
                ));
            }
            let fast = if b["fast"].as_bool() == Some(true) {
                "on"
            } else {
                "off"
            };
            let difficulty = python_str(&item.difficulty);
            out.push(format!(
                "box {} ({} {} d{difficulty}): {what}{tag}, fast {fast}",
                item.id, item.project, item.category
            ));
        }
        let strings = |key: &str| -> Vec<String> {
            result[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        };
        let explore = strings("explore");
        if !explore.is_empty() {
            out.push(format!("explore: {}", explore.join(", ")));
        }
        out.extend(
            strings("held")
                .iter()
                .map(|k| format!("no explore and no new lead: {k} (successor due)")),
        );
        out.extend(strings("needs"));
        out
    }
}

fn shown(value: &Value) -> String {
    python_str(value)
}

pub fn divergence(result: &Value, expect: &Value) -> Option<String> {
    let entries = |key: &str| expect[key].as_object().cloned().unwrap_or_default();
    for (n, s) in entries("states") {
        let got = &result["accounts"][&n]["state"];
        if got != &s {
            return Some(format!(
                "account {n}: state {}, expected {}",
                shown(got),
                shown(&s)
            ));
        }
    }
    for (key, label) in [("tiers", "tier"), ("accounts", "account"), ("fast", "fast")] {
        for (b, want) in entries(key) {
            let got = &result["boxes"][&b][label];
            if got != &want {
                return Some(format!(
                    "box {b}: {label} {}, expected {}",
                    shown(got),
                    shown(&want)
                ));
            }
        }
    }
    for (p, want) in entries("capacity") {
        let entry = &result["capacity"][&p];
        let got = entry
            .get("fallback")
            .cloned()
            .unwrap_or_else(|| json!("available"));
        if got != want {
            return Some(format!(
                "capacity {p}: {entry}, expected fallback {}",
                shown(&want)
            ));
        }
    }
    for key in ["explore", "held", "needs", "routes", "warnings"] {
        if let Some(want) = expect.get(key)
            && &result[key] != want
        {
            return Some(format!("{key}: {}, expected {want}", result[key]));
        }
    }
    None
}
