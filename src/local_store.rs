use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{NorthError, NorthResult};

pub(crate) struct LocalStore {
    directory: PathBuf,
    _custody: File,
}

impl LocalStore {
    pub(crate) fn default_root() -> NorthResult<PathBuf> {
        if let Some(root) = std::env::var_os("XDG_STATE_HOME").filter(|root| !root.is_empty()) {
            return Ok(PathBuf::from(root).join("north-v2"));
        }
        let home = std::env::var_os("HOME").ok_or_else(|| NorthError::Configuration("No home directory for saved workspace data".into()))?;
        Ok(PathBuf::from(home).join(".local/state/north-v2"))
    }

    pub(crate) fn open(root: &Path, cwd: &Path) -> NorthResult<Self> {
        let canonical = cwd.canonicalize()?;
        let key = format!("{:x}", Sha256::digest(canonical.as_os_str().as_encoded_bytes()));
        let directory = root.join(key);
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&directory)?;
        let metadata = fs::symlink_metadata(&directory)?;
        if !metadata.is_dir() || metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
            return Err(NorthError::Configuration("Saved workspace directory must be private and belong to the current user".into()));
        }
        let custody = OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .mode(0o600).open(directory.join("custody"))?;
        custody.try_lock().map_err(|error| NorthError::Configuration(format!(
            "Could not open saved workspace data at {}: another North may already have it open ({error})", directory.display()
        )))?;
        Ok(Self { directory, _custody: custody })
    }

    pub(crate) fn read(&self) -> NorthResult<Option<Vec<u8>>> {
        let mut file = match File::open(self.directory.join("world")) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let length = usize::try_from(file.metadata()?.len()).map_err(|_| NorthError::Configuration(
            "Saved workspace data is too large to open on this machine; the file has been preserved".into()
        ))?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(|_| NorthError::Configuration(
            "There is not enough memory to open saved workspace data; the file has been preserved".into()
        ))?;
        bytes.resize(length, 0);
        file.read_exact(&mut bytes)?;
        if file.read(&mut [0])? != 0 {
            return Err(NorthError::Configuration("Saved workspace data changed while opening; the file has been preserved".into()));
        }
        Ok(Some(bytes))
    }

    fn atomic_write(&self, name: &str, bytes: &[u8]) -> NorthResult<()> {
        let mut file = tempfile::NamedTempFile::new_in(&self.directory)?;
        file.write_all(bytes)?;
        file.as_file().sync_all()?;
        file.persist(self.directory.join(name)).map_err(|error| error.error)?;
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }

    pub(crate) fn checkpoint(&self, bytes: &[u8]) -> NorthResult<()> {
        self.atomic_write("world", bytes)
    }

    pub(crate) fn save_image(&self, number: u64, path: &Path) -> NorthResult<()> {
        self.atomic_write(&format!("image-{number}"), &fs::read(path)?)
    }

    pub(crate) fn load_image(&self, number: u64) -> NorthResult<tempfile::NamedTempFile> {
        let mut source = File::open(self.directory.join(format!("image-{number}")))?;
        let mut file = tempfile::Builder::new().prefix("north-image-").suffix(".png").tempfile()?;
        std::io::copy(&mut source, &mut file)?;
        Ok(file)
    }
}
