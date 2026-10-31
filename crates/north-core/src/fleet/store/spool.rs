use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::NorthResult;

use super::FleetRow;

pub const SPOOL_DIR: &str = "spool";
const ROW: &str = "json";
const REJECTED: &str = "rejected";

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

    pub fn set_aside(&self, ids: &[String]) -> NorthResult<()> {
        ids.iter().try_for_each(|id| FleetRow::check_id(id))?;
        let aside = self.dir.join(REJECTED);
        fs::create_dir_all(&aside)?;
        for id in ids {
            match fs::rename(self.path(id)?, aside.join(format!("{id}.{ROW}"))) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
                _ => {}
            }
        }
        Ok(())
    }

    pub fn holds(&self, id: &str) -> bool {
        self.path(id).is_ok_and(|path| path.is_file())
    }

    pub fn pending(&self) -> NorthResult<Vec<FleetRow>> {
        self.files()?.iter().filter_map(|path| read(path).transpose()).map(|bytes| Ok(serde_json::from_slice(&bytes?)?)).collect()
    }

    pub fn listing(&self) -> NorthResult<Vec<Spooled>> {
        let mut spooled = Vec::new();
        for path in self.files()? {
            let Some(bytes) = read(&path)? else { continue };
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else { continue };
            let row = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            spooled.push(Spooled { id: id.to_owned(), row });
        }
        Ok(spooled)
    }

    fn files(&self) -> NorthResult<Vec<PathBuf>> {
        let mut paths: Vec<PathBuf> = match fs::read_dir(&self.dir) {
            Ok(entries) => entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == ROW)).collect(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        paths.sort();
        Ok(paths)
    }

    fn path(&self, id: &str) -> NorthResult<PathBuf> {
        FleetRow::check_id(id)?;
        Ok(self.dir.join(format!("{id}.{ROW}")))
    }
}

fn read(path: &Path) -> NorthResult<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Spooled {
    pub id: String,
    pub row: Value,
}
