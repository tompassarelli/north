use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, SessionRecord};
use crate::fleet::runtime::Runtime;

/// The one effort a Sonnet session runs at: the workers policy staffs Sonnet only at high.
fn pinned(model: Option<&str>) -> Option<Effort> {
    model.is_some_and(|model| model.to_lowercase().contains("sonnet")).then_some(Effort::High)
}

/// The effort a launch records: none unless asked, high for Sonnet whatever the account's default,
/// the session's seeded draw from a runtime's effort arms, and refused by a runtime that takes none,
/// for Sonnet at any other effort, or outside a runtime's arms.
pub fn taken(runtime: &dyn Runtime, model: Option<&str>, effort: Option<Effort>, seed: &str) -> NorthResult<Option<Effort>> {
    let arms = runtime.effort_arms();
    match (effort, pinned(model)) {
        (Some(_), _) if !runtime.takes_effort() => Err(NorthError::Usage(format!("{} sessions take no --effort; launch a claude session to set reasoning effort", runtime.provider().label()))),
        (Some(asked), Some(only)) if asked != only => Err(NorthError::Usage(format!("{} runs only at {} effort, not {}; drop --effort or pass --effort {}", model.unwrap_or_default(), only.label(), asked.label(), only.label()))),
        (Some(asked), None) if !arms.is_empty() && !arms.contains(&asked) => Err(NorthError::Usage(format!("{} runs only at {} effort, not {}; drop --effort", runtime.provider().label(), arms.iter().map(|effort| effort.label()).collect::<Vec<_>>().join(" or "), asked.label()))),
        (_, Some(only)) if runtime.takes_effort() => Ok(Some(only)),
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
/// a stored Sonnet effort other than high is restored to high.
pub fn revived(record: &SessionRecord, runtime: &dyn Runtime, running: bool, effort: Option<Effort>) -> NorthResult<Option<Effort>> {
    match effort {
        None => Ok(pinned(record.model.as_deref()).filter(|_| runtime.takes_effort()).or(record.effort)),
        Some(_) if running => Err(NorthError::Usage(format!("{} is running; stop it before reviving it with --effort", record.name))),
        Some(_) => taken(runtime, record.model.as_deref(), effort, &record.name),
    }
}
