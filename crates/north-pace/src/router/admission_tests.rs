use super::choose::Bands;
use super::ctx::{Ctx, Tier, WorkBox};
use serde_json::{Value, json};

const NOW: &str = "2026-10-17T03:00Z";

fn ctx(value: Value) -> Ctx {
    serde_json::from_value(value).unwrap()
}

fn tier(key: &str, rank: f64, start: f64) -> Tier {
    serde_json::from_value(json!({"key": key, "rank": rank, "start": start})).unwrap()
}

fn item(category: &str, difficulty: f64, sensitivity: &str) -> WorkBox {
    serde_json::from_value(json!({"id": "b", "project": "p", "category": category, "difficulty": difficulty, "sensitivity": sensitivity})).unwrap()
}

// KEEP(contract): a private box is never admitted to a gemini tier (private-code handling)
#[test]
fn a_private_box_is_never_admitted_to_a_gemini_tier() {
    let ctx = ctx(json!({"now": NOW, "success": {"gemini:pro|feature": 100}}));
    let gemini = tier("gemini:pro", 0.0, 0.0);
    assert!(!ctx.admit(&gemini, &item("feature", 50.0, "private"), &Bands::new()));
    assert!(ctx.admit(&gemini, &item("feature", 50.0, "public"), &Bands::new()));
}
