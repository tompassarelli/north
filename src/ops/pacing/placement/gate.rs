//! Claude-only launch placement for `north fleet launch`: the cloud credit first for code-only public
//! work, then the local Claude account with the most headroom. Pace lines and weekly projections
//! never hold a launch; an account is out only when a window is exhausted or burning away.
use super::super::storage::{self, load, rows};
use super::super::{sampling, target};
use super::{credit, facets, field, strings};
use serde_json::Value;
use toml_edit::DocumentMut;

const FIVE_HOUR_CEILING: f64 = 95.0;
const WEEK_CEILING: f64 = 100.0;
const RUNAWAY_HORIZON_H: f64 = 2.0;
const PUBLIC_REPOS: [&str; 3] = ["north", "wisp", "smashcraft"];
const LOCAL_NEEDS: [&str; 12] = [
    "warcraft", "wc3", "client", "native", "sign-ins", "private", "local-state", "tui", "fleet", "live", "desktop", "laptop",
];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Verdict {
    pub account: String,
    pub available: bool,
    pub headroom: f64,
    pub line: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    Cloud(String),
    Account(String),
    Held(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Decision {
    pub target: Target,
    pub code_only: bool,
    pub surface: String,
    pub verdicts: Vec<Verdict>,
}

impl Decision {
    pub(crate) fn best_account(&self) -> Option<&Verdict> {
        self.verdicts
            .iter()
            .filter(|v| v.available)
            .max_by(|a, b| a.headroom.total_cmp(&b.headroom).then(b.account.cmp(&a.account)))
    }

    pub(crate) fn verdict(&self, account: &str) -> Option<&Verdict> {
        self.verdicts.iter().find(|v| v.account == account)
    }
}

fn hours_to_reset(window: &Value, at: f64) -> f64 {
    window["resets_at"]
        .as_str()
        .and_then(target::seconds)
        .map(|reset| ((reset - at) / 3600.0).max(0.0))
        .or_else(|| window["reset_h"].as_f64())
        .unwrap_or(0.0)
}

fn window_refusal(window: &Value, at: f64) -> Option<String> {
    let name = window["window"].as_str().unwrap_or("window");
    let used = window["used_pct"].as_f64()?;
    let ceiling = if name == "week" { WEEK_CEILING } else { FIVE_HOUR_CEILING };
    if used >= ceiling {
        return Some(format!("exhausted: {name} {used:.0}% >= {ceiling:.0}%"));
    }
    let burn = window["burn_pct_h"].as_f64().unwrap_or(0.0).max(0.0);
    let horizon = hours_to_reset(window, at).min(RUNAWAY_HORIZON_H);
    let projected = used + burn * horizon;
    (projected > 100.0).then(|| {
        format!("runaway: {name} {used:.0}% + {burn:.1}%/h x {horizon:.1}h = {projected:.0}% > 100% before reset")
    })
}

fn headroom(windows: &[Value]) -> f64 {
    windows
        .iter()
        .filter_map(|w| {
            let used = w["used_pct"].as_f64()?;
            Some(if w["window"] == "week" { WEEK_CEILING - used } else { FIVE_HOUR_CEILING - used })
        })
        .min_by(f64::total_cmp)
        .unwrap_or(0.0)
}

fn usage(windows: &[Value]) -> String {
    windows
        .iter()
        .filter_map(|w| Some(format!("{} {:.0}%", w["window"].as_str()?, w["used_pct"].as_f64()?)))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn verdict(account: &str, control: &Value, at: f64) -> Verdict {
    let windows = rows(&control["windows"]);
    let unavailable = |line: String| Verdict { account: account.into(), available: false, headroom: 0.0, line };
    if windows.iter().all(|w| w["used_pct"].as_f64().is_none()) {
        return unavailable(format!("{account} unavailable: no live quota sample"));
    }
    if let Some(reason) = windows.iter().find_map(|w| window_refusal(w, at)) {
        return unavailable(format!("{account} unavailable: {reason}"));
    }
    let room = headroom(windows);
    Verdict {
        account: account.into(),
        available: true,
        headroom: room,
        line: format!("{account} available: {} ({room:.0}% headroom)", usage(windows)),
    }
}

fn repository(text: &str) -> String {
    let declared = field(text, "Repository");
    let source = if declared.is_empty() {
        field(text, "Item").split('#').next().unwrap_or_default()
    } else {
        declared.split_whitespace().next().unwrap_or_default()
    };
    let last = source.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    last.trim_end_matches(".git").trim().to_lowercase()
}

fn words(text: &str, key: &str) -> Vec<String> {
    field(text, key)
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || c == ',' || c == '|' || c == '/')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A box is code-only only when its brief declares a code `Surface:`, needs nothing local and names
/// a public repository; anything undeclared stays local.
pub(crate) fn surface(text: &str, cfg: &DocumentMut) -> (bool, String) {
    let surface = words(text, "Surface");
    if surface.is_empty() {
        return (false, "no Surface: declared, so local".into());
    }
    let (_, native, needs) = facets(text);
    let local = surface
        .iter()
        .chain(needs.iter())
        .chain(words(text, "Needs").iter())
        .find(|w| LOCAL_NEEDS.contains(&w.as_str()))
        .cloned();
    if let Some(need) = local.or_else(|| native.then(|| "native".to_owned())) {
        return (false, format!("needs {need}, so local"));
    }
    let repo = repository(text);
    let rules = cfg.get("placement");
    let private = strings(rules.and_then(|p| p.get("private_repos")));
    let mut public = strings(rules.and_then(|p| p.get("public_repos")));
    if public.is_empty() {
        public = PUBLIC_REPOS.iter().map(|s| (*s).to_owned()).collect();
    }
    if repo.is_empty() || private.contains(&repo) || !public.contains(&repo) {
        let name = if repo.is_empty() { "no repository" } else { repo.as_str() };
        return (false, format!("{name} is not a public repository, so local"));
    }
    (true, format!("code-only box on public {repo}"))
}

fn claude_controls(gate: &Value) -> Vec<(String, &Value)> {
    gate["claude"]["controls"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(name, _)| name.starts_with("claude:"))
        .map(|(name, control)| (name.clone(), control))
        .collect()
}

pub(crate) fn decide(text: &str, cfg: &DocumentMut, gate: &Value, credit: (bool, String), at: f64) -> Decision {
    let (code_only, surface) = surface(text, cfg);
    let verdicts: Vec<Verdict> = claude_controls(gate).into_iter().map(|(name, c)| verdict(&name, c, at)).collect();
    let mut decision = Decision { target: Target::Held(String::new()), code_only, surface, verdicts };
    decision.target = if code_only && credit.0 {
        Target::Cloud(credit.1)
    } else if let Some(best) = decision.best_account() {
        Target::Account(best.account.clone())
    } else if decision.verdicts.is_empty() {
        Target::Held("no live Claude quota in usage-gate.json; run pace-control".into())
    } else {
        Target::Held("every Claude account exhausted or runaway".into())
    };
    decision
}

/// `target anthropic-cloud (credit; surface)`, `target claude:c (surface)` or `target held: why`.
pub(crate) fn summary(decision: &Decision) -> String {
    match &decision.target {
        Target::Cloud(credit) => format!("target anthropic-cloud ({credit}; {})", decision.surface),
        Target::Account(account) => format!("target {account} ({})", decision.surface),
        Target::Held(why) => format!("target held: {why}"),
    }
}

/// The live decision: South's orchestration.toml, the gate pace-control last wrote and the credit.
pub(crate) fn live(text: &str) -> Result<Decision, String> {
    let cfg = sampling::config()?;
    let at = std::env::var("PLACEMENT_NOW")
        .ok()
        .and_then(|s| target::seconds(&s))
        .unwrap_or_else(storage::now);
    let gate = load(&storage::directory().join("usage-gate.json"));
    Ok(decide(text, &cfg, &gate, credit(&cfg, at), at))
}
