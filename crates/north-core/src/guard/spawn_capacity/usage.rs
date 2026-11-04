use super::briefs::{text, truthy};
use super::{World, state_dir};
use serde_json::{Value, json};

pub(super) fn verdict(world: &impl World, provider: &str, granted: bool) -> Result<Value, String> {
    if let Some(verdict) = read(world, provider, granted)? {
        return Ok(verdict);
    }
    let Some((code, stdout)) = world.refresh(provider) else {
        return Ok(json!({"refusal": format!("Hold: {provider} usage refresh unavailable; needs a reading <=120s old.")}));
    };
    let mut verdict = read(world, provider, granted)?
        .unwrap_or_else(|| json!({"refusal": format!("Hold: {provider} usage remains stale after refresh; needs a reading <=120s old.")}));
    if code == 3 && !stdout.trim().is_empty() {
        verdict["refusal"] = json!(stdout.trim());
    }
    Ok(verdict)
}

fn read(world: &impl World, provider: &str, granted: bool) -> Result<Option<Value>, String> {
    let Some(all) = std::fs::read_to_string(state_dir(world).join("agents/usage-gate.json")).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()) else {
        return Ok(None);
    };
    crate::platform::reader_stamp::touch(&state_dir(world), "agents/usage-gate.json");
    let Some(mut verdict) = all.as_object().and_then(|all| all.get(provider)).filter(|verdict| verdict.is_object()).cloned() else {
        return Ok(None);
    };
    let refusals = &verdict["refusals"];
    if truthy(refusals) && !refusals.is_object() {
        return Err("refusals is not a mapping".into());
    }
    let billed = world.billed(provider);
    let refusal = billed.as_deref().and_then(|account| refusals.get(account)).unwrap_or(&verdict["refusal"]).clone();
    verdict["refusal"] = refusal;
    if granted && verdict["refusal"].is_null() {
        return Ok(Some(verdict));
    }
    let (Some(epoch), Some(stale)) = (verdict["epoch"].as_f64(), verdict["stale_min"].as_f64()) else {
        return Ok(None);
    };
    let now = world.now();
    if !(0.0..(stale * 60.0).min(120.0)).contains(&(now - epoch)) {
        return Ok(None);
    }
    if truthy(&verdict["pace_control"]) {
        let account = match billed {
            Some(account) => account,
            None => pace_account(&verdict, provider)?,
        };
        let Some(control) = control(&verdict, &account, provider)? else {
            return Ok(None);
        };
        match world.pace(&json!({"op": "fresh", "control": control, "at": now})) {
            Ok(fresh) if truthy(&fresh) => {}
            _ => return Ok(None),
        }
    }
    Ok(Some(verdict))
}

pub(super) fn pace_account(verdict: &Value, provider: &str) -> Result<String, String> {
    let pace = &verdict["pace"];
    if truthy(pace) && !pace.is_object() {
        return Err("pace is not a mapping".into());
    }
    let account = &pace["account"];
    Ok(if truthy(account) { text(account) } else { provider.to_owned() })
}

pub(super) fn control(verdict: &Value, account: &str, provider: &str) -> Result<Option<Value>, String> {
    let controls = &verdict["controls"];
    if truthy(controls) && !controls.is_object() {
        return Err("controls is not a mapping".into());
    }
    if let Some(found) = controls.get(account).filter(|found| truthy(found)) {
        return Ok(Some(found.clone()));
    }
    Ok(controls.get(provider).filter(|found| !found.is_null()).cloned())
}
