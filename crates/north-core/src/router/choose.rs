use super::brief::{
    brief_header, codex_class, codex_explore_qualifies, codex_qualifies, header_word,
};
use super::ctx::{Ctx, Project, Tier, UNDER, WorkBox, account_of, provider, truthy};
use super::moment::{python_g, python_str};
use super::route::drain_open;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub type Bands = BTreeMap<String, (f64, f64)>;

#[derive(Debug, Clone, Default)]
pub struct Choice {
    pub tier: Option<Tier>,
    pub explore: bool,
    pub wait: Option<String>,
    pub needs: Option<String>,
    pub warn: Option<String>,
}

fn eligible(tier: &Tier, item: &WorkBox) -> bool {
    provider(&tier.key) != "gemini" || item.sensitivity == "public"
}

fn brief(item: &WorkBox) -> &str {
    item.brief.as_deref().unwrap_or("")
}

fn costs_file() -> Option<Vec<Value>> {
    let dir = std::env::var_os("AGENTS_OPERATOR_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join("code/south/main/agents"))
        })?;
    let document: toml_edit::DocumentMut = std::fs::read_to_string(dir.join("costs.toml"))
        .ok()?
        .parse()
        .ok()?;
    super::policy::json_value(document.as_item())["service"]
        .as_array()
        .cloned()
}

fn lever(service: &Value) -> Option<String> {
    let kind = service["kind"].as_str();
    if kind == Some("ai-plan") {
        return None;
    }
    let action = if matches!(kind, Some("cloud" | "gpu-rental"))
        && service["billing"].as_str() != Some("fixed")
    {
        "rent build/test compute"
    } else {
        "reconfigure/swap service"
    };
    let price = service["usd_per_week"].as_f64().map_or_else(
        || "$ unmeasured".to_owned(),
        |amount| format!("current ledger cost ${}/week", python_g(amount)),
    );
    Some(format!(
        "{action}: {} {price}, speedup unmeasured",
        python_str(&service["id"])
    ))
}

pub fn missed_target(project: &str, target: &str, services: Option<&[Value]>) -> String {
    let loaded = services.is_none().then(|| costs_file().unwrap_or_default());
    let services = services.or(loaded.as_deref()).unwrap_or_default();
    let mut levers: Vec<String> = services.iter().filter_map(lever).collect();
    if levers.is_empty() {
        levers.push(
            "rent build/test compute / reconfigure/swap service: $ unmeasured, speedup unmeasured"
                .into(),
        );
    }
    format!(
        "Blocked: decision for Tom: {project} projected miss {target}: more budget ({}) / later date / smaller scope",
        levers.join("; ")
    )
}

impl Ctx {
    fn category(&self, item: &WorkBox) -> Option<String> {
        header_word(&brief_header(brief(item)), "category", &self.knobs.codex)
    }

    pub fn bands(&self) -> Bands {
        let mut tiers: Vec<&Tier> = self.tiers.iter().collect();
        tiers.sort_by(|a, b| a.rank.total_cmp(&b.rank));
        let mut starts: Vec<(String, f64)> = Vec::new();
        let mut floor = f64::NEG_INFINITY;
        for tier in tiers.into_iter().filter(|t| t.start < 100.0) {
            floor = tier.start.max(floor);
            starts.push((tier.key.clone(), floor));
        }
        let mut out = Bands::new();
        for (i, (key, lo)) in starts.iter().enumerate() {
            let lo = if i == 0 { f64::NEG_INFINITY } else { *lo };
            out.insert(
                key.clone(),
                (lo, starts.get(i + 1).map_or(f64::INFINITY, |next| next.1)),
            );
        }
        out
    }

    pub fn admit(&self, tier: &Tier, item: &WorkBox, bands: &Bands) -> bool {
        if !eligible(tier, item) {
            return false;
        }
        let codex = &self.knobs.codex;
        if provider(&tier.key) == "codex"
            && !(codex_qualifies(brief(item), false, codex).0
                && codex_class(self.category(item).as_deref(), codex)
                    .tiers
                    .contains(&tier.key))
        {
            return false;
        }
        if let Some(rate) = self.success.get(&format!("{}|{}", tier.key, item.category)) {
            return *rate >= self.knobs.success_threshold;
        }
        let (lo, hi) = bands
            .get(&tier.key)
            .copied()
            .unwrap_or((f64::INFINITY, f64::INFINITY));
        let difficulty = item.difficulty.as_f64().unwrap_or(f64::NAN);
        lo <= difficulty && difficulty < hi
    }

    pub fn successor_soon(&self, tier: &Tier) -> bool {
        tier.successor.as_ref().is_some_and(|s| {
            s.eta_days <= self.knobs.successor_days && s.p >= self.knobs.successor_p
        })
    }

    fn runs_open(&self, tier: &Tier) -> bool {
        tier.runs.unwrap_or(f64::INFINITY) < self.knobs.explore_runs
    }

    fn codex_explorable(&self, tier: &Tier, item: &WorkBox) -> bool {
        let codex = &self.knobs.codex;
        if !codex_explore_qualifies(brief(item), codex).0 {
            return false;
        }
        let category = self.category(item);
        let class = codex_class(category.as_deref(), codex);
        if class.route == "codex" {
            return self.runs_open(tier);
        }
        let runs = category
            .and_then(|c| tier.class_runs.get(&c).copied())
            .unwrap_or(0.0);
        class.tiers.contains(&tier.key) && runs < self.knobs.codex_explore_min_n
    }

    pub fn explorable(&self, tier: &Tier, item: &WorkBox) -> bool {
        !self.successor_soon(tier)
            && eligible(tier, item)
            && self.account_state(&account_of(tier)) == Some(UNDER)
            && if provider(&tier.key) == "codex" {
                self.codex_explorable(tier, item)
            } else {
                self.runs_open(tier)
            }
    }

    fn priced(&self) -> Vec<(f64, f64, &Tier)> {
        let accounts: BTreeSet<String> = self.tiers.iter().map(account_of).collect();
        let prices: BTreeMap<String, f64> = accounts
            .into_iter()
            .map(|n| (n.clone(), self.price(&n)))
            .collect();
        self.tiers
            .iter()
            .map(|t| (prices[&account_of(t)], t.rank, t))
            .collect()
    }

    fn missed(&self, item: &WorkBox, project: &Project, target: &str) -> String {
        missed_target(
            project.name.as_deref().unwrap_or(&item.project),
            target,
            self.services.as_deref(),
        )
    }

    pub fn choose(&self, item: &WorkBox, explore_slot: bool) -> Choice {
        let fallback = Project::default();
        let project = self.projects.get(&item.project).unwrap_or(&fallback);
        if truthy(&item.local) && !truthy(&self.lease) {
            return Choice {
                wait: Some("capacity lease".into()),
                ..Choice::default()
            };
        }
        let priced = self.priced();
        if explore_slot {
            let pool = priced
                .iter()
                .filter(|(p, _, t)| *p != f64::INFINITY && self.explorable(t, item));
            if let Some((_, _, tier)) = pool.min_by(|a, b| a.1.total_cmp(&b.1)) {
                return Choice {
                    tier: Some((*tier).clone()),
                    explore: true,
                    ..Choice::default()
                };
            }
        }
        let bands = self.bands();
        let admitted: Vec<&(f64, f64, &Tier)> = priced
            .iter()
            .filter(|(_, _, t)| self.admit(t, item, &bands))
            .collect();
        let open = admitted.iter().filter(|(p, _, t)| {
            *p != f64::INFINITY && self.launch_refusal(&account_of(t)).is_none()
        });
        let Some((_, _, tier)) = open.min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)))
        else {
            return self.closed(
                item,
                project,
                admitted.iter().any(|(p, _, _)| *p != f64::INFINITY),
            );
        };
        let name = account_of(tier);
        let mut out = Choice {
            tier: Some((*tier).clone()),
            warn: self.pace_warning(item, project, &name),
            ..Choice::default()
        };
        if admitted.len() == 1
            && project.u >= self.knobs.late
            && self.billing(&name, "fast") != "plan"
        {
            let target = project
                .target
                .as_ref()
                .map_or_else(|| "its target".to_owned(), python_str);
            out.needs = Some(self.missed(item, project, &target));
        }
        out
    }

    fn closed(&self, item: &WorkBox, project: &Project, priced: bool) -> Choice {
        if !priced {
            return Choice {
                needs: Some(format!(
                    "Blocked: decision for Tom: no under-pace plan-billed tier admits {} ({}, {} d{}); bill usage credits for it?",
                    item.id,
                    item.project,
                    item.category,
                    python_str(&item.difficulty)
                )),
                ..Choice::default()
            };
        }
        match project.target.as_ref().filter(|t| truthy(t)) {
            Some(target) => Choice {
                needs: Some(self.missed(item, project, &python_str(target))),
                ..Choice::default()
            },
            None => Choice {
                wait: Some("account exhausted".into()),
                ..Choice::default()
            },
        }
    }

    fn pace_warning(&self, item: &WorkBox, project: &Project, name: &str) -> Option<String> {
        if self.account_state(name) == Some(UNDER) {
            return None;
        }
        let why = if self.enforced(name) && drain_open(project, name, self.now) {
            format!(
                "{} drains it under Tom's grant until {}",
                project.name.as_deref().unwrap_or(&item.project),
                project.target.as_ref().map_or_else(String::new, python_str)
            )
        } else {
            let start = self
                .enforce_from(name)
                .map_or_else(|| "unset".to_owned(), |s| s.iso());
            format!("pace enforced from {start}")
        };
        Some(format!("account {name} at or over pace; admitted, {why}"))
    }
}
