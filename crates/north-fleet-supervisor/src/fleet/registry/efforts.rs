use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, SessionRecord};
use crate::fleet::runtime::Runtime;

fn sonnet(model: Option<&str>) -> bool {
    model.is_some_and(|model| model.to_lowercase().contains("sonnet"))
}

fn sonnet_effort(effort: Effort) -> bool {
    matches!(effort, Effort::Medium | Effort::High)
}

/// The effort the launcher admits for a Sonnet session: the one asked when it is medium or high, else
/// high. Dispatch reads it too, so a launch it builds never carries an effort the launcher refuses.
pub fn admitted(model: Option<&str>, asked: Option<Effort>) -> Option<Effort> {
    sonnet(model).then(|| asked.filter(|effort| sonnet_effort(*effort)).unwrap_or(Effort::High))
}

/// The effort a launch records: none unless asked, high for Sonnet when none is asked, medium or high
/// for Sonnet when asked, the session's seeded draw from a runtime's effort arms, and refused by a
/// runtime that takes none, for Sonnet at any other effort, or outside a runtime's arms.
pub fn taken(runtime: &dyn Runtime, model: Option<&str>, effort: Option<Effort>, seed: &str) -> NorthResult<Option<Effort>> {
    let arms = runtime.effort_arms();
    match (effort, sonnet(model)) {
        (Some(_), _) if !runtime.takes_effort() => Err(NorthError::Usage(format!("{} sessions take no --effort; launch a claude session to set reasoning effort", runtime.provider().label()))),
        (Some(asked), true) if !sonnet_effort(asked) => Err(NorthError::Usage(format!("{} runs only at medium or high effort, not {}; drop --effort or pass --effort medium or high", model.unwrap_or_default(), asked.label()))),
        (Some(asked), false) if !arms.is_empty() && !arms.contains(&asked) => Err(NorthError::Usage(format!("{} runs only at {} effort, not {}; drop --effort", runtime.provider().label(), arms.iter().map(|effort| effort.label()).collect::<Vec<_>>().join(" or "), asked.label()))),
        (Some(asked), _) => Ok(Some(asked)),
        (None, true) if runtime.takes_effort() => Ok(Some(Effort::High)),
        (None, _) if !arms.is_empty() => Ok(Some(drawn(seed, arms))),
        _ => Ok(effort),
    }
}

/// The arm `seed` draws from `arms`, the same arm on every launch of the session name.
fn drawn(seed: &str, arms: &[Effort]) -> Effort {
    let hash = seed.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    let mixed = (hash ^ (hash >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
    let mixed = (mixed ^ (mixed >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    arms[((mixed ^ (mixed >> 33)) % arms.len() as u64) as usize]
}

/// The effort a revive records: the stored one unless a new one is given, which a running session refuses;
/// a stored Sonnet effort other than medium or high is restored to high.
pub fn revived(record: &SessionRecord, runtime: &dyn Runtime, running: bool, effort: Option<Effort>) -> NorthResult<Option<Effort>> {
    match effort {
        None if sonnet(record.model.as_deref()) => Ok(record.effort.filter(|effort| sonnet_effort(*effort)).or_else(|| runtime.takes_effort().then_some(Effort::High))),
        None => Ok(record.effort),
        Some(_) if running => Err(NorthError::Usage(format!("{} is running; stop it before reviving it with --effort", record.name))),
        Some(_) => taken(runtime, record.model.as_deref(), effort, &record.name),
    }
}
