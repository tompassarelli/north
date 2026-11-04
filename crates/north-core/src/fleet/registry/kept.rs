use std::fs;

use crate::error::NorthResult;
use crate::fleet::store::WaitKey;

use super::Fleet;

const UNSETTLED: &str = ".unsettled-waits.json";

impl Fleet {
    pub fn unsettled_waits(&self) -> Vec<WaitKey> {
        fs::read(self.root.join(UNSETTLED)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    pub fn set_unsettled_waits(&self, waits: &[WaitKey]) -> NorthResult<()> {
        let file = self.root.join(UNSETTLED);
        if waits.is_empty() {
            return match fs::remove_file(&file) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
                _ => Ok(()),
            };
        }
        let scratch = self.root.join(format!("{UNSETTLED}.tmp"));
        fs::write(&scratch, serde_json::to_vec(waits)?)?;
        fs::rename(scratch, file)?;
        Ok(())
    }
}
