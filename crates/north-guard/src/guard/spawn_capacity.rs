use crate::hook::{HookInput, Verdict};
use serde_json::{Value, json};
use north_strategy_types::placement::SpawnCpu;
use std::path::PathBuf;

mod briefs;
mod logs;
mod usage;
mod world;

pub use briefs::Event;
use crate::spend::{self, Placed, Rules, Starter, View};
use toml_edit::DocumentMut;
pub use world::Live;

const PROTECTED_PRESSURE_LIMIT: f64 = 20.0;
const LOCAL_CLAUDE_TYPES: [&str; 8] = ["worker", "worker-high", "worker-xhigh", "worker-haiku", "worker-sonnet", "general-purpose", "Explore", "fork"];
const EXEMPT_MARKER: &str = "[routine:watchdog]";
const UNIT: &str = "spawn-capacity-guard";
const HOOKED: Live = Live;

pub trait World {
    fn now(&self) -> f64;
    fn var(&self, name: &str) -> Option<String>;
    fn probe(&self) -> Option<String>;
    fn fleet(&self) -> Option<String>;
    fn flow(&self, repo: &str) -> Option<String>;
    fn billed(&self, provider: &str) -> Option<String>;
    fn cpus(&self) -> f64;
    fn today(&self) -> String;
}

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off(UNIT) {
        return Verdict::Allow;
    }
    admission(input)
}

pub fn admission(input: &HookInput) -> Verdict {
    match Event::from_hook(input).and_then(|event| decide(&HOOKED, &event)) {
        Some(reason) => Verdict::Deny(reason),
        None => Verdict::Allow,
    }
}

/// What `north hook spawn-gate` prints: whether this host's CPUs admit a spawn, and whether the spend
/// rule admits a subagent on each account.
pub fn gate_summary(world: &impl World) -> Value {
    let cpu = capacity(world).is_none_or(|(provisioned, limit, protected)| provisioned < limit && protected <= PROTECTED_PRESSURE_LIMIT);
    let (view, rules) = spend_view(world);
    let accounts: Vec<Value> = view.accounts.iter().map(|account| json!([account.name, view.admit(&rules, &account.name, Starter::Subagent, Placed::default()).is_ok()])).collect();
    json!({"cpu": cpu, "accounts": accounts, "spend": view.line(&rules)})
}

pub fn decide(world: &impl World, event: &Event) -> Option<String> {
    let reasons = spawn(world, event).ok()?;
    (!reasons.is_empty()).then(|| reasons.join("\n"))
}

/// The money refusal for the billed account, the spend rule for a subagent on it, then the landing
/// flow and CPU capacity.
fn spawn(world: &impl World, event: &Event) -> Result<Vec<String>, String> {
    let provider = event.provider();
    let mut reasons: Vec<String> = usage::money_refusal(world, provider)?.into_iter().collect();
    if let Some(account) = world.billed(provider) {
        let (view, rules) = spend_view(world);
        if let Err(refusal) = view.admit(&rules, &account, Starter::Subagent, Placed::default()) {
            logs::denial(world, event, "spend", Some(&refusal.to_string()));
            reasons.push(refusal.to_string());
        }
    }
    if !event.texts.iter().any(|text| text.contains(EXEMPT_MARKER)) {
        reasons.extend(flow_refusal(world, event));
        reasons.extend(capacity_refusal(world, event));
    }
    Ok(reasons)
}

/// The spend view from this host's state as last sampled; a subagent adds no session, so the active
/// count is not read.
fn spend_view(world: &impl World) -> (View, Rules) {
    let cfg = std::fs::read_to_string(crate::pace::target::policy_path()).ok().and_then(|text| text.parse::<DocumentMut>().ok()).unwrap_or_default();
    let rules = Rules::configured();
    let catalog = crate::platform::harness::document(crate::platform::harness::MODEL_CATALOG);
    (spend::view(&state_dir(world).join("agents"), &rules, 0, world.now(), |name| spend::plan_week(&cfg, catalog, name), |_| true), rules)
}

fn flow_refusal(world: &impl World, event: &Event) -> Option<String> {
    let reason = briefs::flow_gate(world, event)?;
    logs::denial(world, event, "flow", Some(&reason));
    Some(reason)
}

fn capacity_refusal(world: &impl World, event: &Event) -> Option<String> {
    if !event.is_local() || event.texts.iter().any(|text| briefs::LANDING_PATH.is_match(text)) || event.named_lander() {
        return None;
    }
    let (provisioned, limit, protected) = capacity(world)?;
    let rank = fleet_domain_rank(world, event);
    let headroom = rank.as_ref().filter(|rank| rank.1 > 0).map_or(0.0, |_| priority_headroom());
    let ceiling = limit * (1.0 - headroom);
    if provisioned < ceiling && protected <= PROTECTED_PRESSURE_LIMIT {
        return None;
    }
    if let Some(fact) = briefs::urgent_file_fact(world, &event.texts) {
        logs::urgent(world, event, &fact);
        return None;
    }
    if let Some(fact) = urgent_fact(event.texts.iter().map(String::as_str)) {
        logs::urgent(world, event, &fact);
        return None;
    }
    logs::denial(world, event, "capacity", None);
    if provisioned < limit && protected <= PROTECTED_PRESSURE_LIMIT
        && let Some((domain, _, first)) = rank
    {
        let g = briefs::py_g;
        return Some(format!(
            "Domain {domain} ranks below {first} in Tom's priority order, so its local spawns stop at {} of the {} CPUs; {} are committed. Move code-only work to a cloud worker (workers skill) or queue it until a lease ends. Take a real conflict to the proxy, not Tom.",
            g(ceiling), g(limit), g(provisioned)
        ));
    }
    Some(cpu_refusal(world, provisioned, limit, protected))
}

/// The capacity refusal a local `north fleet launch` meets: the same CPU line, pressure limit and
/// remedy as an Agent-tool spawn, passed by an urgent fact in the brief or the launch's `--pin`.
pub fn launch_refusal(world: &impl World, brief: &str) -> Option<String> {
    launch_verdict(&launch_capacity(world)?, brief)
}

/// What the launch refusal reads from the host: committed and available CPUs, protected pressure, and the
/// local workers its text counts, read only when the CPU line refuses.
pub fn launch_capacity(world: &impl World) -> Option<SpawnCpu> {
    let (provisioned, limit, protected) = capacity(world)?;
    let local_workers = refusing(provisioned, limit, protected).then(|| u32::try_from(logs::local_workers(world)).unwrap_or(u32::MAX));
    Some(SpawnCpu { provisioned, limit, protected, local_workers })
}

pub fn launch_verdict(cpu: &SpawnCpu, brief: &str) -> Option<String> {
    if !refusing(cpu.provisioned, cpu.limit, cpu.protected) {
        return None;
    }
    urgent_fact([brief]).is_none().then(|| cpu_refusal_text(cpu.provisioned, cpu.limit, cpu.protected, cpu.local_workers.unwrap_or(0) as usize))
}

fn refusing(provisioned: f64, limit: f64, protected: f64) -> bool {
    !(provisioned < limit && protected <= PROTECTED_PRESSURE_LIMIT)
}

/// The first `CASE=URGENT FACT="..."` fact in the texts, when it is long enough to say why the work can't wait.
fn urgent_fact<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let found = texts.into_iter().find_map(|text| briefs::URGENT.captures(text))?;
    let fact = found.get(1).or(found.get(2)).map_or("", |m| m.as_str()).trim().to_owned();
    (fact.chars().count() >= 15).then_some(fact)
}

fn cpu_refusal(world: &impl World, provisioned: f64, limit: f64, protected: f64) -> String {
    cpu_refusal_text(provisioned, limit, protected, logs::local_workers(world))
}

fn cpu_refusal_text(provisioned: f64, limit: f64, protected: f64, local_workers: usize) -> String {
    let g = briefs::py_g;
    format!(
        "Capacity leases and unleased heavy load commit {} of the {} CPUs the machine can hand out, protected desktop pressure is {}% (limit {}%), and {} local workers are running. Queue this worker until a lease ends, send heavy checks to the farm, or use a cloud worker for code-only work (workers skill). Workers already running are unaffected. For an urgent fix, start the brief with CASE=URGENT FACT=\"<why it can't wait>\". Codex, whose briefs are encrypted, appends \"<epoch>\\t<agent name>\\t<fact>\" to ~/.local/state/agents/spawn-urgent.tsv first.",
        g(provisioned), g(limit), g(protected), g(PROTECTED_PRESSURE_LIMIT), local_workers
    )
}

fn capacity(world: &impl World) -> Option<(f64, f64, f64)> {
    let data: Value = serde_json::from_str(&world.probe()?).ok()?;
    data.as_object()?;
    let provisioned = data["committedBatchCpus"].as_f64()?;
    let limit = data["aggregateCpuLimit"].as_f64().filter(|limit| *limit != 0.0).unwrap_or_else(|| world.cpus());
    let protected = data["protectedCpuSomeAvg10"].as_f64()?;
    Some((provisioned, limit, if data["profile"] == "unattended" { 0.0 } else { protected }))
}

fn priority_headroom() -> f64 {
    let defaults: toml_edit::DocumentMut = north_base::platform::harness::table(north_base::platform::harness::DEFAULTS).parse().expect("harness/defaults.toml parses");
    let value = &defaults["priority_headroom"]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).expect("harness/defaults.toml has priority_headroom")
}

fn fleet_domain_rank(world: &impl World, event: &Event) -> Option<(String, usize, String)> {
    let session = event.raw["session_id"].as_str().filter(|session| !session.is_empty())?;
    let records: Value = serde_json::from_str(&world.fleet()?).ok()?;
    let mut found = None;
    for record in records.as_array()? {
        record.as_object()?;
        if record["conversation"] == session {
            found = Some(record);
            break;
        }
    }
    let task_ref = found?["task_ref"].as_str()?;
    let domain = task_ref.split('#').next().unwrap_or("").rsplit('/').next().unwrap_or("").to_lowercase();
    let policy = world.var("AGENTS_ORCHESTRATION").map(PathBuf::from).unwrap_or_else(|| operator_dir(world).join("orchestration.toml"));
    let document: toml_edit::DocumentMut = std::fs::read_to_string(policy).ok()?.parse().ok()?;
    let mut ranked = Vec::new();
    if let Some(projects) = document.get("projects") {
        for (name, project) in projects.as_table_like()?.iter() {
            let priority = match project.as_table_like()?.get("priority") {
                None => f64::INFINITY,
                Some(item) => item.as_float().or_else(|| item.as_integer().map(|whole| whole as f64))?,
            };
            ranked.push((priority, name.to_lowercase()));
        }
    }
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let order: Vec<String> = ranked.into_iter().map(|(_, name)| name).collect();
    let index = order.iter().position(|name| *name == domain)?;
    Some((domain, index, order[0].clone()))
}

fn operator_dir(world: &impl World) -> PathBuf {
    world.var("AGENTS_OPERATOR_DIR").map(PathBuf::from).unwrap_or_else(|| home(world).join("code/south/main/agents"))
}

fn home(world: &impl World) -> PathBuf {
    PathBuf::from(world.var("HOME").unwrap_or_default())
}

fn capacity_home(world: &impl World) -> PathBuf {
    world.var("SPAWN_CAPACITY_HOME").map_or_else(|| home(world), PathBuf::from)
}

fn state_dir(world: &impl World) -> PathBuf {
    match world.var("SPAWN_CAPACITY_HOME") {
        Some(capacity) => PathBuf::from(capacity).join(".local/state"),
        None => world.var("XDG_STATE_HOME").map_or_else(|| home(world).join(".local/state"), PathBuf::from),
    }
}
