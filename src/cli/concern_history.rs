use north_concerns::history::{Recorded, Sample, Store};
use north_foundation::foundation::clock::Timestamp;

pub(crate) struct NoHistory;

impl Store for NoHistory {
    fn record(&self, _: &[Sample]) -> Result<(), String> {
        Ok(())
    }

    fn read(&self, _: &str, _: Timestamp) -> Result<Vec<Recorded>, String> {
        Ok(Vec::new())
    }
}
