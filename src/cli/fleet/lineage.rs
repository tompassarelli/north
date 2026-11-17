//! Who hears a launched session's reports: the coordinator for a launch from outside the fleet.

use crate::fleet::model::Role;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::COORDINATOR;

pub(in crate::cli) fn from_coordinator(fleet: &Fleet, caller: Option<&str>) -> bool {
    caller.is_some_and(|caller| caller == COORDINATOR || fleet.role_of(caller) == Some(Role::Coordinator))
}
