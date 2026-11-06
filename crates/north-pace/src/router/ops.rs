use super::brief::codex_box_refusal;
use super::ctx::{Ctx, Knobs};
use super::plan::divergence;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    MoneyRefusal {
        ctx: Ctx,
        account: String,
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

const OPS: [&str; 7] = [
    "money_refusal",
    "fast",
    "route",
    "plan",
    "plan_lines",
    "divergence",
    "codex_box",
];

fn respond(request: Request) -> Result<Value, String> {
    Ok(match request {
        Request::MoneyRefusal { ctx, account } => json!(ctx.money_refusal(&account)),
        Request::Fast { ctx, account } => json!(ctx.fast(&account)),
        Request::Route { ctx, provider } => json!({
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
