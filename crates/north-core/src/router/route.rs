use super::ctx::{Account, Ctx, Project, UNDER, exhausted, expiry, provider, used};
use super::moment::{Moment, target_open};
use crate::pace::exhaustion;
use serde_json::{Value, json};
use std::cmp::Ordering;

fn by_expiry(ctx: &Ctx, a: &str, b: &str) -> Ordering {
    expiry(&ctx.accounts[a])
        .total_cmp(&expiry(&ctx.accounts[b]))
        .then_with(|| a.cmp(b))
}

pub fn drain_open(project: &Project, name: &str, now: Moment) -> bool {
    let granted = [name, provider(name)].iter().any(|key| {
        project
            .drain
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|grant| !grant.trim().is_empty())
    });
    granted && target_open(project.target.as_ref().unwrap_or(&Value::Null), now)
}

impl Ctx {
    fn names(&self, p: &str) -> impl Iterator<Item = &String> {
        self.accounts.keys().filter(move |n| provider(n) == p)
    }

    pub fn enforce_from(&self, name: &str) -> Option<Moment> {
        self.pace
            .as_ref()?
            .enforce_from
            .get(name)
            .and_then(Moment::of)
    }

    pub fn enforced(&self, name: &str) -> bool {
        self.enforce_from(name)
            .is_some_and(|start| self.now >= start)
    }

    pub fn launch_refusal(&self, name: &str) -> Option<String> {
        self.accounts
            .get(name)?
            .windows
            .iter()
            .find_map(|(window, w)| exhaustion::refusal(window, 100.0 - w.headroom, w.burn_pct_h))
    }

    fn routable(&self, p: &str) -> impl Iterator<Item = &String> {
        self.names(p)
            .filter(|n| self.accounts[*n].auto_route && self.price(n) != f64::INFINITY)
    }

    pub fn route_account(&self, p: &str) -> Option<String> {
        self.routable(p)
            .filter(|n| self.account_state(n) == Some(UNDER))
            .min_by(|a, b| by_expiry(self, a, b))
            .cloned()
    }

    pub fn provider_account(&self, p: &str) -> String {
        if let Some(routed) = self.route_account(p) {
            return routed;
        }
        self.names(p)
            .filter(|n| self.accounts[*n].auto_route)
            .min_by(|a, b| {
                self.price(a)
                    .total_cmp(&self.price(b))
                    .then_with(|| by_expiry(self, a, b))
            })
            .cloned()
            .unwrap_or_else(|| p.to_owned())
    }

    pub fn route(&self, p: &str) -> Option<String> {
        let rank = |name: &String| {
            (
                self.account_state(name) != Some(UNDER),
                exhausted(self.accounts.get(name)),
            )
        };
        self.routable(p)
            .filter(|n| self.launch_refusal(n).is_none())
            .min_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| by_expiry(self, a, b)))
            .cloned()
    }

    pub fn route_label(&self, p: &str) -> String {
        let Some(name) = self.route(p) else {
            return "none launchable".into();
        };
        if self.account_state(&name) == Some(UNDER) {
            return name;
        }
        let start = self
            .enforce_from(&name)
            .map_or_else(|| "unset".to_owned(), Moment::iso);
        format!("{name} (at or over pace, enforced from {start})")
    }

    pub fn pace_reading(&self, p: &str, account: Option<&str>) -> Option<Value> {
        let names: Vec<&String> = self
            .names(p)
            .filter(|n| account.is_none_or(|a| a == n.as_str()))
            .collect();
        if let Some(stale) = names
            .iter()
            .find(|n| self.stale(&self.accounts[n.as_str()]))
        {
            return Some(
                json!({"account": stale, "stale": self.stale_warning(stale, &self.accounts[stale.as_str()])}),
            );
        }
        let first = names.iter().min_by(|a, b| by_expiry(self, a, b))?;
        let name = account
            .map(str::to_owned)
            .or_else(|| self.route_account(p))
            .unwrap_or_else(|| first.to_string());
        let acct = &self.accounts[&name];
        let m = self.metrics(acct);
        if m.used_pct.is_none() {
            return Some(json!({"account": name, "stale": self.stale_warning(&name, acct)}));
        }
        Some(
            json!({"account": name, "state": m.state, "used_pct": m.used_pct, "pace_pct": m.pace_pct,
                    "exhausted": exhausted(Some(acct)), "stale": null}),
        )
    }

    pub fn fast(&self, name: &str) -> bool {
        let Some(acct) = self.accounts.get(name) else {
            return false;
        };
        if self.billing(name, "fast") != "plan" || self.price(name) == f64::INFINITY {
            return false;
        }
        self.metrics(acct).state == UNDER
            && acct
                .windows
                .values()
                .all(|w| w.headroom > self.knobs.fast_floor)
    }

    pub fn cap_pct(&self, name: &str) -> &Value {
        if self.overflow_money(name) {
            &self.knobs.money_ceiling
        } else {
            &self.knobs.plan_stop
        }
    }

    pub fn money_refusal(&self, name: &str) -> Option<String> {
        let Some(acct) = self.accounts.get(name) else {
            return Some(format!(
                "no usage snapshot for {name} could be read (`agents usage --refresh` shows why)"
            ));
        };
        if self.price(name) != f64::INFINITY {
            return None;
        }
        Some(self.money_reason(name, acct))
    }

    fn money_reason(&self, name: &str, acct: &Account) -> String {
        if self.stale(acct) {
            let age = acct
                .age_min
                .as_f64()
                .map_or_else(|| "unknown".to_owned(), |age| format!("{age:.0}"));
            return format!("{name}'s usage snapshot is {age} minutes old even after a refresh");
        }
        let standard = self.billing(name, "standard");
        if standard != "plan" {
            return format!("the catalog bills {name}'s standard usage as {standard}");
        }
        let flags = [
            ("hasCredits", &acct.has_credits),
            ("extra_usage", &acct.extra_usage),
            ("spend", &acct.spend),
        ];
        let live: Vec<&str> = flags
            .iter()
            .filter(|(_, v)| super::ctx::truthy(v))
            .map(|(k, _)| *k)
            .collect();
        let live = if live.is_empty() {
            "its overflow".to_owned()
        } else {
            live.join(", ")
        };
        format!(
            "{name} is at {:.0}% of a plan window (ceiling {}%) and {live} would bill money past it",
            used(acct),
            self.knobs.money_ceiling
        )
    }
}
