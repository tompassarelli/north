use super::brief::codex_box_refusal;
use super::ctx::{Ctx, Knobs, Project, provider, used};
use super::moment::Moment;
use super::plan::divergence;
use super::route::drain_open;
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Drain {
        project: Project,
        account: String,
        now: Option<Moment>,
    },
    MoneyRefusal {
        ctx: Ctx,
        account: String,
    },
    Gate {
        ctx: Ctx,
        provider: String,
    },
    Fast {
        ctx: Ctx,
        account: String,
    },
    Route {
        ctx: Ctx,
        provider: String,
    },
    Plan {
        ctx: Ctx,
    },
    PlanLines {
        ctx: Ctx,
        result: Option<Value>,
    },
    Divergence {
        result: Value,
        expect: Value,
    },
    CodexBox {
        brief: String,
    },
}

const OPS: [&str; 9] = [
    "drain",
    "money_refusal",
    "gate",
    "fast",
    "route",
    "plan",
    "plan_lines",
    "divergence",
    "codex_box",
];

fn gate(ctx: &Ctx, p: &str) -> Value {
    let names: Vec<&String> = ctx.accounts.keys().filter(|n| provider(n) == p).collect();
    let accounts: Map<String, Value> = names
        .iter()
        .map(|n| {
            (
                (*n).clone(),
                json!({"used_pct": used(&ctx.accounts[n.as_str()]), "cap_pct": ctx.cap_pct(n)}),
            )
        })
        .collect();
    let paces: Map<String, Value> = names
        .iter()
        .map(|n| ((*n).clone(), json!(ctx.pace_reading(p, Some(n)))))
        .collect();
    json!({"pace": ctx.pace_reading(p, None), "paces": paces, "accounts": accounts})
}

fn respond(request: Request) -> Result<Value, String> {
    Ok(match request {
        Request::Drain {
            project,
            account,
            now,
        } => {
            let now = now.unwrap_or_else(Moment::now);
            let target = project.target.clone().unwrap_or(Value::Null);
            json!({"drain_open": drain_open(&project, &account, now), "target_open": super::moment::target_open(&target, now)})
        }
        Request::MoneyRefusal { ctx, account } => json!(ctx.money_refusal(&account)),
        Request::Gate { ctx, provider } => gate(&ctx, &provider),
        Request::Fast { ctx, account } => json!(ctx.fast(&account)),
        Request::Route { ctx, provider } => json!({
            "under_pace": ctx.route_account(&provider),
            "route": ctx.route(&provider),
            "fallback": ctx.provider_account(&provider),
        }),
        Request::Plan { ctx } => {
            serde_json::to_value(ctx.plan()?).map_err(|error| error.to_string())?
        }
        Request::PlanLines { ctx, result } => {
            json!({"head": ctx.head_lines(), "tail": result.map(|r| ctx.box_lines(&r)).unwrap_or_default()})
        }
        Request::Divergence { result, expect } => json!(divergence(&result, &expect)),
        Request::CodexBox { brief } => json!(codex_box_refusal(&brief, &Knobs::default().codex)),
    })
}

pub fn answer(input: &Value) -> Option<Result<Value, String>> {
    let op = input.get("op")?.as_str()?;
    if !OPS.contains(&op) {
        return None;
    }
    Some(
        Request::deserialize(input)
            .map_err(|error| format!("{op}: {error}"))
            .and_then(respond),
    )
}
