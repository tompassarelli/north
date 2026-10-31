use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::NorthResult;

use super::FleetRow;

pub const SPOOL_DIR: &str = "spool";
const ROW: &str = "json";

pub struct Spool {
    dir: PathBuf,
}

impl Spool {
    pub fn at(root: &Path) -> Self {
        Self { dir: root.join(SPOOL_DIR) }
    }

    pub fn put(&self, row: &FleetRow) -> NorthResult<()> {
        row.validate()?;
        fs::create_dir_all(&self.dir)?;
        let staged = self.dir.join(format!(".{}.tmp", row.id));
        fs::write(&staged, serde_json::to_vec(row)?)?;
        fs::rename(staged, self.path(&row.id)?)?;
        Ok(())
    }

    pub fn withdraw(&self, id: &str) -> NorthResult<()> {
        match fs::remove_file(self.path(id)?) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }

    pub fn withdraw_all(&self, ids: &[String]) -> NorthResult<()> {
        ids.iter().try_for_each(|id| FleetRow::check_id(id))?;
        ids.iter().try_for_each(|id| self.withdraw(id))
    }

    pub fn holds(&self, id: &str) -> bool {
        self.path(id).is_ok_and(|path| path.is_file())
    }

    pub fn pending(&self) -> NorthResult<Vec<FleetRow>> {
        let mut paths: Vec<PathBuf> = match fs::read_dir(&self.dir) {
            Ok(entries) => entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == ROW)).collect(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        paths.sort();
        paths.iter().map(|path| Ok(serde_json::from_slice(&fs::read(path)?)?)).collect()
    }

    fn path(&self, id: &str) -> NorthResult<PathBuf> {
        FleetRow::check_id(id)?;
        Ok(self.dir.join(format!("{id}.{ROW}")))
    }
}
