use crate::error::{NorthError, NorthResult};

use super::Fleet;

/// A session this many parent links below the lead may not launch sessions of its own.
const DEPTH_CAP: usize = 2;

impl Fleet {
    /// Parent links between `session` and the lead at the top of its chain; a parent whose record is gone still counts as a link.
    pub fn levels_below_lead(&self, session: &str) -> usize {
        let mut levels = 0;
        let mut current = session.to_owned();
        while levels < DEPTH_CAP {
            let Some(parent) = self.record(&current).ok().and_then(|record| record.parent) else { break };
            levels += 1;
            current = parent;
        }
        levels
    }

    pub fn admit_launch_from(&self, caller: Option<&str>) -> NorthResult<()> {
        let Some(caller) = caller.filter(|name| !name.is_empty()) else { return Ok(()) };
        match self.levels_below_lead(caller) {
            levels if levels >= DEPTH_CAP => Err(NorthError::Refused(format!(
                "{caller} is {levels} levels below the lead and may not launch sessions; report to your parent so it launches the work"
            ))),
            _ => Ok(()),
        }
    }
}
