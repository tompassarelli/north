use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{NorthError, NorthResult};

use super::FleetRow;

pub const SPOOL_DIR: &str = "spool";
pub const SESSIONS_DIR: &str = "sessions";
const ROW: &str = "json";
const REJECTED: &str = "rejected";

/// The fleet's spool: the shared location the fleet's own processes write, and beneath `sessions` one location per session, whose rows the tick reads as that session's and nobody else's.
pub struct Spool {
    dir: PathBuf,
    writes: PathBuf,
}

impl Spool {
    pub fn at(root: &Path) -> Self {
        let dir = root.join(SPOOL_DIR);
        Self { writes: dir.clone(), dir }
    }

    /// The spool as `session` writes it, into its own location.
    pub fn of(root: &Path, session: &str) -> NorthResult<Self> {
        let dir = root.join(SPOOL_DIR);
        Ok(Self { writes: dir.join(SESSIONS_DIR).join(location(session)?), dir })
    }

    /// The directory this spool writes its rows into.
    pub fn location(&self) -> &Path {
        &self.writes
    }

    pub fn put(&self, row: &FleetRow) -> NorthResult<()> {
        row.validate()?;
        FleetRow::check_id(&row.id)?;
        fs::create_dir_all(&self.writes)?;
        north_publish::publish(&self.writes.join(format!("{}.{ROW}", row.id)), &serde_json::to_vec(row)?, 0o666)?;
        Ok(())
    }

    pub fn withdraw(&self, id: &str) -> NorthResult<()> {
        for path in self.places(id)? {
            match fs::remove_file(path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
                _ => {}
            }
        }
        Ok(())
    }

    pub fn withdraw_all(&self, ids: &[String]) -> NorthResult<()> {
        ids.iter().try_for_each(|id| FleetRow::check_id(id))?;
        ids.iter().try_for_each(|id| self.withdraw(id))
    }

    pub fn set_aside(&self, ids: &[String]) -> NorthResult<()> {
        ids.iter().try_for_each(|id| FleetRow::check_id(id))?;
        let mut paths = Vec::new();
        for id in ids {
            paths.extend(self.places(id)?);
        }
        self.set_aside_paths(&paths)
    }

    pub fn holds(&self, id: &str) -> bool {
        self.places(id).is_ok_and(|places| places.iter().any(|path| path.is_file()))
    }

    pub fn pending(&self) -> NorthResult<Vec<FleetRow>> {
        let mut rows = Vec::new();
        let mut refused = Vec::new();
        for found in self.files()? {
            let Some(bytes) = read(&found.path)? else { continue };
            match serde_json::from_slice::<FleetRow>(&bytes) {
                Ok(row) => match found.admit(row) {
                    Ok(row) => rows.push(row),
                    Err(why) => {
                        eprintln!("north fleet: refused spooled row {}: {why}", found.path.display());
                        refused.push(found.path);
                    }
                },
                Err(error) => {
                    eprintln!("north fleet: set aside spooled row {} that does not parse ({error})", found.path.display());
                    refused.push(found.path);
                }
            }
        }
        self.set_aside_paths(&refused)?;
        Ok(rows)
    }

    pub fn listing(&self) -> NorthResult<Vec<Spooled>> {
        let mut spooled = Vec::new();
        let mut refused = Vec::new();
        for found in self.files()? {
            let Some(bytes) = read(&found.path)? else { continue };
            let mut row = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            if let Some(writer) = &found.writer {
                let refusal = match FleetRow::deserialize(&row) {
                    Ok(parsed) => found.admit(parsed).err(),
                    Err(_) => found.misfiled(None),
                };
                if let Some(why) = refusal {
                    eprintln!("north fleet: refused spooled row {}: {why}", found.path.display());
                    refused.push(found.path);
                    continue;
                }
                if let Some(fields) = row.as_object_mut() {
                    fields.insert("spooled_by".into(), Value::String(writer.clone()));
                }
            }
            spooled.push(Spooled { id: found.id, row });
        }
        self.set_aside_paths(&refused)?;
        Ok(spooled)
    }

    fn set_aside_paths(&self, paths: &[PathBuf]) -> NorthResult<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let aside = self.dir.join(REJECTED);
        fs::create_dir_all(&aside)?;
        for path in paths {
            let Some(name) = path.file_name() else { continue };
            match fs::rename(path, aside.join(name)) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
                _ => {}
            }
        }
        Ok(())
    }

    fn files(&self) -> NorthResult<Vec<Found>> {
        let mut found: Vec<Found> = rows_in(&self.dir)?.into_iter().map(|path| Found::new(path, None)).collect();
        for (session, location) in self.locations()? {
            found.extend(rows_in(&location)?.into_iter().map(|path| Found::new(path, Some(session.clone()))));
        }
        found.sort_by(|left, right| (&left.id, &left.path).cmp(&(&right.id, &right.path)));
        let ids: Vec<String> = found.iter().map(|found| found.id.clone()).collect();
        for (index, found) in found.iter_mut().enumerate() {
            found.shared = ids.get(index.wrapping_sub(1)) == Some(&found.id) || ids.get(index + 1) == Some(&found.id);
        }
        Ok(found)
    }

    fn locations(&self) -> NorthResult<Vec<(String, PathBuf)>> {
        let entries = match fs::read_dir(self.dir.join(SESSIONS_DIR)) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        Ok(entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| entry.file_name().into_string().ok().filter(|name| location(name).is_ok()).map(|name| (name, entry.path())))
            .collect())
    }

    fn places(&self, id: &str) -> NorthResult<Vec<PathBuf>> {
        FleetRow::check_id(id)?;
        let file = format!("{id}.{ROW}");
        let mut places = vec![self.dir.join(&file)];
        places.extend(self.locations()?.into_iter().map(|(_, location)| location.join(&file)));
        Ok(places)
    }
}

fn location(session: &str) -> NorthResult<&str> {
    if session.is_empty() || session.starts_with('.') || session.contains('/') || session.chars().any(char::is_whitespace) {
        return Err(NorthError::Usage(format!("not a session name: {session:?}")));
    }
    Ok(session)
}

struct Found {
    path: PathBuf,
    id: String,
    writer: Option<String>,
    shared: bool,
}

impl Found {
    fn new(path: PathBuf, writer: Option<String>) -> Self {
        let id = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        Self { path, id, writer, shared: false }
    }

    fn misfiled(&self, row: Option<&FleetRow>) -> Option<String> {
        self.writer.as_ref()?;
        if FleetRow::check_id(&self.id).is_err() {
            return Some(format!("{} is not a row id", self.id));
        }
        if self.shared {
            return Some(format!("row {} is also pending in another spool location", self.id));
        }
        row.filter(|row| row.id != self.id).map(|row| format!("it is filed as {} but names row {}", self.id, row.id))
    }

    fn admit(&self, row: FleetRow) -> Result<FleetRow, String> {
        let Some(writer) = &self.writer else { return Ok(row) };
        if let Some(why) = self.misfiled(Some(&row)).or_else(|| row.refused_from(writer)) {
            return Err(why);
        }
        Ok(FleetRow { spooled_by: Some(writer.clone()), ..row })
    }
}

fn rows_in(dir: &Path) -> NorthResult<Vec<PathBuf>> {
    match fs::read_dir(dir) {
        Ok(entries) => Ok(entries.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file())).map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == ROW)).collect()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
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
