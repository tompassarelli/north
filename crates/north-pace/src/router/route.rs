use super::ctx::{Account, Ctx, expiring, provider, used};
use std::cmp::Ordering;

fn by_room(ctx: &Ctx, a: &str, b: &str) -> Ordering {
    expiring(&ctx.accounts[b])
        .total_cmp(&expiring(&ctx.accounts[a]))
        .then_with(|| a.cmp(b))
}

impl Ctx {
    fn names(&self, p: &str) -> impl Iterator<Item = &String> {
        self.accounts.keys().filter(move |n| provider(n) == p)
    }

    pub fn provider_limit(&self, name: &str) -> Option<String> {
        self.accounts
            .get(name)?
            .windows
            .iter()
            .find(|(_, w)| w.headroom <= 0.0)
            .map(|(window, _)| format!("{name} is at its provider's {window} limit"))
    }

    fn routable(&self, p: &str) -> impl Iterator<Item = &String> {
        self.names(p)
            .filter(|n| self.accounts[*n].auto_route && self.price(n) != f64::INFINITY)
    }

    pub fn provider_account(&self, p: &str) -> String {
        if let Some(routed) = self.route(p) {
            return routed;
        }
        self.names(p)
            .filter(|n| self.accounts[*n].auto_route)
            .min_by(|a, b| {
                self.price(a)
                    .total_cmp(&self.price(b))
                    .then_with(|| by_room(self, a, b))
            })
            .cloned()
            .unwrap_or_else(|| p.to_owned())
    }

    pub fn route(&self, p: &str) -> Option<String> {
        self.routable(p)
            .filter(|n| self.provider_limit(n).is_none())
            .min_by(|a, b| by_room(self, a, b))
            .cloned()
    }

    pub fn route_label(&self, p: &str) -> String {
        self.route(p).unwrap_or_else(|| "none launchable".into())
    }

    pub fn fast(&self, name: &str) -> bool {
        let Some(acct) = self.accounts.get(name) else {
            return false;
        };
        if self.billing(name, "fast") != "plan" || self.price(name) == f64::INFINITY {
            return false;
        }
        acct.windows
            .values()
            .all(|w| w.headroom > self.knobs.fast_floor)
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
