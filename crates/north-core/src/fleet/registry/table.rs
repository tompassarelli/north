use crate::error::NorthResult;
use crate::fleet::store::{SessionRow, cache_sessions};

use super::Fleet;

impl Fleet {
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn cache_sessions(&self, rows: &[SessionRow]) -> NorthResult<()> {
        cache_sessions(&self.root, rows)
    }
}
