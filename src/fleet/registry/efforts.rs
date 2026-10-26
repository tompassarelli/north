use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, SessionRecord};
use crate::fleet::runtime::Runtime;

/// The effort a launch records: none unless asked, and refused by a runtime that takes none.
pub(super) fn taken(runtime: &dyn Runtime, effort: Option<Effort>) -> NorthResult<Option<Effort>> {
    match effort {
        Some(_) if !runtime.takes_effort() => Err(NorthError::Usage(format!("{} sessions take no --effort; launch a claude session to set reasoning effort", runtime.provider().label()))),
        _ => Ok(effort),
    }
}

/// The effort a revive records: the stored one unless a new one is given, which a running session refuses.
pub(super) fn revived(record: &SessionRecord, runtime: &dyn Runtime, running: bool, effort: Option<Effort>) -> NorthResult<Option<Effort>> {
    match effort {
        None => Ok(record.effort),
        Some(_) if running => Err(NorthError::Usage(format!("{} is running; stop it before reviving it with --effort", record.name))),
        Some(_) => taken(runtime, effort),
    }
}
