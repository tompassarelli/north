use super::Fleet;
use crate::fleet::model::SessionRecord;
use crate::fleet::records::past_records;

impl Fleet {
    pub fn past_sessions(&self) -> Vec<SessionRecord> {
        past_records(&self.root)
    }
}
