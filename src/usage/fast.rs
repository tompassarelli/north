//! `north usage --fast PROVIDER`: whether routing turns plan-billed fast on for a new lead on the provider while it is under pace.
use rusqlite::Connection;
use serde_json::{Value, json};

use super::{Host, Places, gate, refresh, store};
use crate::router::ops;

pub(super) fn decide(db: &Connection, source: &store::Source, places: &Places, provider: &str, cached: bool, host: &Host) -> Result<(String, u8), String> {
    if !cached {
        refresh::read_provider(db, source, provider, host)?;
    }
    let now = super::clock();
    let inputs = gate::Inputs::load(&places.policy)?;
    let context = inputs.context(db, &super::readings(db, &places.policy)?, provider, now)?;
    let local = gate::local_account(provider, &inputs.cfg);
    let name = gate::account_name(&context, provider, local.as_deref());
    match ops::answer(&json!({"op": "fast", "ctx": context, "account": name})) {
        Some(Ok(answer)) if truthy(&answer) => Ok(("on".into(), 0)),
        Some(Ok(_)) => Ok(("off".into(), 1)),
        Some(Err(error)) => Err(error),
        None => Err("routing has no fast op".into()),
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}
