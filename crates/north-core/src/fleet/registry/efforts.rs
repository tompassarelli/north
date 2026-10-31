use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, SessionRecord};
use crate::fleet::runtime::Runtime;

/// The one effort a Sonnet session runs at: the workers policy staffs Sonnet only at high.
fn pinned(model: Option<&str>) -> Option<Effort> {
    model.is_some_and(|model| model.to_lowercase().contains("sonnet")).then_some(Effort::High)
}

/// The effort a launch records: none unless asked, high for Sonnet whatever the account's default,
/// and refused by a runtime that takes none or for Sonnet at any other effort.
pub(super) fn taken(runtime: &dyn Runtime, model: Option<&str>, effort: Option<Effort>) -> NorthResult<Option<Effort>> {
    match (effort, pinned(model)) {
        (Some(_), _) if !runtime.takes_effort() => Err(NorthError::Usage(format!("{} sessions take no --effort; launch a claude session to set reasoning effort", runtime.provider().label()))),
        (Some(asked), Some(only)) if asked != only => Err(NorthError::Usage(format!("{} runs only at {} effort, not {}; drop --effort or pass --effort {}", model.unwrap_or_default(), only.label(), asked.label(), only.label()))),
        (_, Some(only)) if runtime.takes_effort() => Ok(Some(only)),
        _ => Ok(effort),
    }
}

/// The effort a revive records: the stored one unless a new one is given, which a running session refuses;
/// a stored Sonnet effort other than high is restored to high.
pub(super) fn revived(record: &SessionRecord, runtime: &dyn Runtime, running: bool, effort: Option<Effort>) -> NorthResult<Option<Effort>> {
    match effort {
        None => Ok(pinned(record.model.as_deref()).filter(|_| runtime.takes_effort()).or(record.effort)),
        Some(_) if running => Err(NorthError::Usage(format!("{} is running; stop it before reviving it with --effort", record.name))),
        Some(_) => taken(runtime, record.model.as_deref(), effort),
    }
}
