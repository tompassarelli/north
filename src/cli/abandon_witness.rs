use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::land_abandon::{ScratchPush, Witness};

pub(super) struct PushWitness;

impl Witness for PushWitness {
    fn live_sessions(&self) -> Option<BTreeSet<String>> {
        None
    }

    fn pushes(&self) -> Vec<ScratchPush> {
        let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from);
        let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state");
        let log = north_base::platform::push_ledger::log(&state.unwrap_or_else(home));
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split('\t').collect();
                let field = |index: usize| fields.get(index).copied().unwrap_or_default().to_owned();
                Some(ScratchPush { at_s: fields.first()?.parse().ok()?, repository: field(1), branch: field(2), oid: field(3), session: field(5) })
            })
            .collect()
    }
}
