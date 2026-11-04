use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, ProviderName, SessionRecord};
use crate::fleet::records::write_record;

use super::{Fleet, efforts};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Choices {
    pub provider: ProviderName,
    pub models: Vec<String>,
    pub takes_effort: bool,
}

impl Fleet {
    /// The models and efforts each provider accepts for a session, as `relaunch` checks them.
    pub fn choices(&self) -> Vec<Choices> {
        self.providers()
            .into_iter()
            .filter_map(|provider| {
                let runtime = self.runtimes.get(&provider).ok()?;
                Some(Choices { provider, models: runtime.models().iter().map(|model| (*model).to_owned()).collect(), takes_effort: runtime.takes_effort() })
            })
            .collect()
    }
}

impl Fleet {
    /// Restarts a session on the same conversation under another model or effort. Both are checked as a launch checks them before the session is stopped, so a refused change leaves it running.
    pub fn relaunch(&self, name: &str, model: Option<&str>, effort: Option<Effort>) -> NorthResult<SessionRecord> {
        self.relaunch_with(name, model, effort, |held| self.revive_held(held, None, None, None))
    }

    pub(super) fn relaunch_with(&self, name: &str, model: Option<&str>, effort: Option<Effort>, restart: impl FnOnce(&str) -> NorthResult<SessionRecord>) -> NorthResult<SessionRecord> {
        let record = self.record(name)?;
        if model.is_none() && effort.is_none() {
            return Err(NorthError::Usage("relaunch takes a model, an effort or both".into()));
        }
        let runtime = self.runtime(&record)?;
        if let Some(model) = model.filter(|model| !runtime.models().contains(model)) {
            return Err(NorthError::Usage(format!("{} has no model {model}; choose one of {}", record.provider.label(), runtime.models().join(", "))));
        }
        let mut changed = SessionRecord { model: model.map(str::to_owned).or_else(|| record.model.clone()), ..record.clone() };
        changed.effort = efforts::revived(&changed, runtime.as_ref(), false, effort)?;
        self.refuse_beside_interactive(&record)?;
        self.stop(name)?;
        let _start = self.start_lock(name)?;
        let mut stopped = self.record(name)?;
        (stopped.model, stopped.effort) = (changed.model, changed.effort);
        write_record(&self.session_dir(name)?, &stopped)?;
        restart(name)
    }
}
