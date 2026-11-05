use crate::hook::{HookInput, Verdict};
use serde_json::{Value, json};
use std::path::PathBuf;

mod briefs;
mod logs;
mod usage;
mod world;

pub use briefs::Event;
use briefs::truthy;
pub use world::Live;

const PROTECTED_PRESSURE_LIMIT: f64 = 20.0;
const LOCAL_CLAUDE_TYPES: [&str; 8] = ["worker", "worker-high", "worker-xhigh", "worker-haiku", "worker-sonnet", "general-purpose", "Explore", "fork"];
const EXEMPT_MARKER: &str = "[routine:watchdog]";
const UNIT: &str = "spawn-capacity-guard";

pub trait World {
    fn now(&self) -> f64;
    fn var(&self, name: &str) -> Option<String>;
    fn probe(&self) -> Option<String>;
    fn fleet(&self) -> Option<String>;
    fn flow(&self, repo: &str) -> Option<String>;
    fn refresh(&self, provider: &str) -> Option<(i32, String)>;
    fn pace(&self, request: &Value) -> Result<Value, String>;
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
    match Event::from_hook(input).and_then(|event| decide(&Live, &event)) {
        Some(reason) => Verdict::Deny(reason),
        None => Verdict::Allow,
    }
}

pub fn confirm_native(input: &HookInput) -> Verdict {
    if input.data["hook_event_name"] == "PostToolUse" && input.data["tool_name"].as_str().is_some_and(|tool| tool.ends_with("spawn_agent"))
        && let Some(event) = Event::from_hook(input)
    {
        let _ = Live.pace(&json!({
            "op": "confirm", "directory": state_dir(&Live).join("agents"), "session": event.raw["session_id"],
            "tool_use_id": event.raw["tool_use_id"], "launched": true,
        }));
    }
    Verdict::Allow
}

pub fn gate_summary(world: &impl World) -> Value {
    let cpu = capacity(world).is_none_or(|(provisioned, limit, protected)| provisioned < limit && protected <= PROTECTED_PRESSURE_LIMIT);
    let verdict = usage::verdict(world, "claude", false).ok().unwrap_or(Value::Null);
    let open = verdict["refusal"].is_null();
    let accounts: Vec<Value> = verdict["paces"].as_object().into_iter().flatten().map(|(account, _)| json!([account, open])).collect();
    json!({"cpu": cpu, "accounts": accounts})
}

pub fn decide(world: &impl World, event: &Event) -> Option<String> {
    let reasons = spawn(world, event).ok()?;
    if let Some(hold) = reasons.iter().find(|reason| reason.starts_with("Hold:")) {
        let account = world.billed(event.provider()).unwrap_or_else(|| event.provider().to_owned());
        let _ = world.pace(&json!({
            "op": "expired_grant_hold", "directory": state_dir(world).join("agents"), "account": account,
            "session": event.session(), "reason": hold, "task": event.task(),
        }));
    }
    (!reasons.is_empty()).then(|| reasons.join("\n"))
}

fn spawn(world: &impl World, event: &Event) -> Result<Vec<String>, String> {
    let provider = event.provider();
    let agents = state_dir(world).join("agents");
    let billed = world.billed(provider);
    let first = billed.clone().unwrap_or_else(|| provider.to_owned());
    let grant = world.pace(&json!({"op": "grant_for", "directory": agents, "account": first, "session": event.session(), "task": event.task()}))?;
    let granted = truthy(&grant);
    let verdict = usage::verdict(world, provider, granted)?;
    let mut reasons = Vec::new();
    if truthy(&verdict["refusal"]) {
        reasons.push(briefs::text(&verdict["refusal"]));
    }
    let name = match billed {
        Some(account) => Ok(account),
        None => usage::pace_account(&verdict, provider),
    };
    let paced = truthy(&verdict["pace_control"]);
    match name.clone().and_then(|name| pace_hold(world, event, &verdict, &name, paced && !granted)) {
        Ok(hold) => reasons.extend(hold),
        Err(error) if reasons.is_empty() => return Err(error),
        Err(_) => {}
    }
    if !event.texts.iter().any(|text| text.contains(EXEMPT_MARKER)) {
        reasons.extend(flow_refusal(world, event));
        reasons.extend(capacity_refusal(world, event));
    }
    if !reasons.is_empty() || !(paced || granted) {
        return Ok(reasons);
    }
    let reserved = world.pace(&json!({
        "op": "reserve", "path": agents.join("usage-gate.json"), "provider": provider, "account": name?,
        "session": event.session(), "task": event.task(), "tool_use_id": event.raw["tool_use_id"],
    }))?;
    Ok(truthy(&reserved).then(|| briefs::text(&reserved)).into_iter().collect())
}

fn pace_hold(world: &impl World, event: &Event, verdict: &Value, name: &str, holds: bool) -> Result<Option<String>, String> {
    if !holds {
        return Ok(None);
    }
    let provider = event.provider();
    let Some(control) = usage::control(verdict, name, provider)? else {
        return Ok(Some(format!("Hold: {name} pace controller has no fresh account reading; refresh before spawning.")));
    };
    let hold = world.pace(&json!({"op": "refusal", "account": name, "control": control, "now": world.now()}))?;
    if !truthy(&hold) {
        return Ok(None);
    }
    let hold = briefs::text(&hold);
    logs::denial(world, event, "projection", Some(&hold));
    Ok(Some(hold))
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
    let (provisioned, limit, protected) = capacity(world)?;
    if provisioned < limit && protected <= PROTECTED_PRESSURE_LIMIT {
        return None;
    }
    urgent_fact([brief]).is_none().then(|| cpu_refusal(world, provisioned, limit, protected))
}

/// The first `CASE=URGENT FACT="..."` fact in the texts, when it is long enough to say why the work can't wait.
fn urgent_fact<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let found = texts.into_iter().find_map(|text| briefs::URGENT.captures(text))?;
    let fact = found.get(1).or(found.get(2)).map_or("", |m| m.as_str()).trim().to_owned();
    (fact.chars().count() >= 15).then_some(fact)
}

fn cpu_refusal(world: &impl World, provisioned: f64, limit: f64, protected: f64) -> String {
    let g = briefs::py_g;
    format!(
        "Capacity leases and unleased heavy load commit {} of the {} CPUs the machine can hand out, protected desktop pressure is {}% (limit {}%), and {} local workers are running. Queue this worker until a lease ends, send heavy checks to the farm, or use a cloud worker for code-only work (workers skill). Workers already running are unaffected. For an urgent fix, start the brief with CASE=URGENT FACT=\"<why it can't wait>\". Codex, whose briefs are encrypted, appends \"<epoch>\\t<agent name>\\t<fact>\" to ~/.local/state/agents/spawn-urgent.tsv first.",
        g(provisioned), g(limit), g(protected), g(PROTECTED_PRESSURE_LIMIT), logs::local_workers(world)
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
    let defaults: toml_edit::DocumentMut = include_str!("../../../../harness/defaults.toml").parse().expect("harness/defaults.toml parses");
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
