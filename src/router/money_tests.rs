use super::ctx::{Ctx, account_of};
use serde_json::{Value, json};
use toml_edit::DocumentMut;

fn catalog_tiers() -> Vec<Value> {
    let bands: DocumentMut = include_str!("fixtures/policy/orchestration.toml")
        .parse()
        .unwrap();
    let catalog: DocumentMut = include_str!("../../harness/model-catalog.toml")
        .parse()
        .unwrap();
    catalog["models"]
        .as_table()
        .unwrap()
        .iter()
        .filter(|(_, model)| model.get("model").is_some())
        .enumerate()
        .map(|(rank, (key, model))| {
            let start = bands["tiers"]
                .get(key)
                .and_then(|tier| tier["start"].as_integer())
                .unwrap_or(100);
            json!({"key": key, "model": model["model"].as_str(), "effort": model["effort"].as_str(),
                   "rank": rank, "start": start, "runs": if rank % 3 == 0 { 5 } else { 50 }})
        })
        .collect()
}

fn bills_money(ctx: &Ctx, name: &str, billing: &serde_json::Map<String, Value>) -> bool {
    let acct = &ctx.accounts[name];
    let standard_is_plan = billing.get("standard").and_then(Value::as_str) == Some("plan");
    if acct.age_min.as_f64().unwrap() > ctx.knobs.stale_min || !standard_is_plan {
        return true;
    }
    let overflow_free = matches!(billing.get("overflow").and_then(Value::as_str), Some("plan" | "blocked"))
        && !(acct.has_credits.as_bool().unwrap() || acct.extra_usage.as_bool().unwrap() || acct.spend.as_bool().unwrap());
    let used = acct.windows.values().map(|w| 100.0 - w.headroom).fold(f64::MIN, f64::max);
    used >= ctx.knobs.money_ceiling.as_f64().unwrap() && !overflow_free
}

#[test]
fn no_choice_over_the_catalog_tiers_ever_bills_money() {
    let tiers = catalog_tiers();
    let success: Value = tiers
        .iter()
        .map(|t| (format!("{}|any", t["key"].as_str().unwrap()), json!(100)))
        .collect::<serde_json::Map<_, _>>()
        .into();
    let accounts: Vec<String> = {
        let mut names: Vec<String> = tiers
            .iter()
            .map(|t| t["key"].as_str().unwrap().split(':').next().unwrap().to_owned())
            .collect();
        names.sort();
        names.dedup();
        names
    };
    let projects: Value = (0..3)
        .flat_map(|i| [1.0, 1.6].map(|u| (format!("project-{i}-{u}"), json!({"u": u}))))
        .collect::<serde_json::Map<_, _>>()
        .into();
    let shapes = [("public", 10, "any"), ("private", 60, "band"), ("client", 95, "band"), ("public", 40, "band")];
    let boxes: Vec<Value> = projects
        .as_object()
        .unwrap()
        .keys()
        .enumerate()
        .flat_map(|(j, project)| {
            [0, 2].map(|i| {
                let (sensitivity, difficulty, category) = shapes[(i + j) % 4];
                json!({"id": format!("{project}-{i}"), "project": project, "category": category,
                       "difficulty": difficulty, "sensitivity": sensitivity})
            })
        })
        .collect();
    let modes = ["standard", "fast", "ultrafast", "overflow"];
    let values = [Some("plan"), Some("money"), Some("blocked"), None];
    let (mut decisions, mut picks) = (0, 0);
    for bill in billing_product(&[
        &[Some("plan"), Some("money"), None],
        &[Some("plan"), Some("money"), None],
        &[Some("plan"), None],
        &values,
    ]) {
        let billing: serde_json::Map<_, _> = modes
            .iter()
            .zip(&bill)
            .filter_map(|(mode, value)| value.map(|v| ((*mode).to_owned(), json!(v))))
            .collect();
        for credits in [false, true] {
            for stale in [false, true] {
                for pct in [0.0, 94.0, 95.0, 100.0] {
                    let account = |name: &str| {
                        json!({"windows": {"5h": {"headroom": 100.0 - pct, "reset_in": 2, "beta": 1},
                                           "week": {"headroom": 100.0 - pct, "reset_in": 20, "beta": 0.5, "window_h": 168,
                                                    "resets_at": "2026-10-12T08:00Z", "as_of": "2026-10-11T12:00Z"}},
                               "hasCredits": credits, "extra_usage": false, "spend": false,
                               "age_min": if stale { 45 } else { 5 }, "provider": name,
                               "target_left": if name == "codex" { 15 } else { 5 }, "usd_per_week": 50})
                    };
                    let ctx: Ctx = serde_json::from_value(json!({
                        "now": "2026-10-11T12:00Z", "tiers": tiers, "success": success, "lease": true,
                        "projects": projects,
                        "accounts": accounts.iter().map(|n| (n.clone(), account(n))).collect::<serde_json::Map<_, _>>(),
                        "billing": accounts.iter().map(|n| (n.clone(), json!(billing))).collect::<serde_json::Map<_, _>>(),
                    }))
                    .unwrap();
                    for item in &boxes {
                        let item = serde_json::from_value(item.clone()).unwrap();
                        for slot in [false, true] {
                            decisions += 1;
                            let Some(tier) = ctx.choose(&item, slot).tier else { continue };
                            picks += 1;
                            let name = account_of(&tier);
                            assert!(
                                !bills_money(&ctx, &name, &billing),
                                "picked money: {} box {} billing {billing:?} credits {credits} stale {stale} used {pct}",
                                tier.key, item.id
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(picks > 0 && decisions > picks, "{picks} of {decisions}");
}

fn billing_product<'a>(axes: &[&'a [Option<&'a str>]]) -> Vec<Vec<Option<&'a str>>> {
    axes.iter().fold(vec![vec![]], |rows, axis| {
        rows.iter()
            .flat_map(|row| {
                axis.iter().map(move |value| {
                    let mut next = row.clone();
                    next.push(*value);
                    next
                })
            })
            .collect()
    })
}
