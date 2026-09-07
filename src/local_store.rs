use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use clause_package::AtomPayloadSegment;

use crate::error::{NorthError, NorthResult};

pub(crate) struct LocalStore {
    directory: PathBuf,
    _custody: File,
    // Retaining the immutable allocation makes its address a non-reusable key.
    chunks: BTreeMap<(usize, usize), (AtomPayloadSegment, [u8; 32])>,
    current_chunks: BTreeSet<[u8; 32]>,

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
        Ok(Self { directory, _custody: custody, chunks: BTreeMap::new(), current_chunks: BTreeSet::new() })
    }

    pub(crate) fn read(&mut self) -> NorthResult<Option<Vec<u8>>> {
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
        if bytes.starts_with(MANIFEST_MAGIC) { return Ok(Some(self.read_manifest(&bytes)?)); }
        Ok(Some(bytes))
    }

    fn read_manifest(&mut self, bytes: &[u8]) -> NorthResult<Vec<u8>> {
        let (body, digest) = bytes.split_at_checked(bytes.len().checked_sub(32).ok_or_else(invalid_manifest)?)
            .ok_or_else(invalid_manifest)?;
        if Sha256::digest(body).as_slice() != digest { return Err(invalid_manifest()); }
        let mut input = &body[MANIFEST_MAGIC.len()..];
        let length = usize::try_from(u64::from_le_bytes(take(&mut input, 8)?.try_into().map_err(|_| invalid_manifest())?))
            .map_err(|_| invalid_manifest())?;
        let mut output = Vec::new();
        let mut current_chunks = BTreeSet::new();
        while !input.is_empty() {
            let tag = take(&mut input, 1)?[0];
            let count = u32::from_le_bytes(take(&mut input, 4)?.try_into().map_err(|_| invalid_manifest())?) as usize;
            if count > length.saturating_sub(output.len()) { return Err(invalid_manifest()); }
            match tag {
                0 => {
                    let value = take(&mut input, count)?;
                    output.try_reserve(value.len()).map_err(|_| invalid_manifest())?;
                    output.extend_from_slice(value);
                }
                1 => {
                    let digest: [u8; 32] = take(&mut input, 32)?.try_into().map_err(|_| invalid_manifest())?;
                    current_chunks.insert(digest);
                    let mut file = File::open(self.chunk_path(&digest))?;
                    if file.metadata()?.len() != count as u64 { return Err(invalid_manifest()); }
                    let start = output.len();
                    output.try_reserve(count).map_err(|_| invalid_manifest())?;
                    output.resize(start + count, 0);
                    file.read_exact(&mut output[start..])?;
                    if file.read(&mut [0])? != 0 || Sha256::digest(&output[start..]).as_slice() != digest {
                        return Err(invalid_manifest());
                    }
                }
                _ => return Err(invalid_manifest()),
            }
        }
        if output.len() != length { return Err(invalid_manifest()); }
        self.current_chunks = current_chunks;
        Ok(output)
    }

    fn chunk_path(&self, digest: &[u8; 32]) -> PathBuf {
        self.directory.join(format!("world-chunk-{}", digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()))
    }

    fn atomic_write(&self, name: &str, bytes: &[u8]) -> NorthResult<()> {
        let mut file = tempfile::NamedTempFile::new_in(&self.directory)?;
        file.write_all(bytes)?;
        file.as_file().sync_all()?;
        file.persist(self.directory.join(name)).map_err(|error| error.error)?;
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }

    pub(crate) fn checkpoint(&mut self, segments: &[AtomPayloadSegment]) -> NorthResult<()> {
        let length = segments.iter().try_fold(0u64, |length, segment| length.checked_add(segment.as_bytes().len() as u64))
            .ok_or_else(invalid_manifest)?;
        let mut manifest = MANIFEST_MAGIC.to_vec();
        manifest.extend_from_slice(&length.to_le_bytes());
        let mut retained = BTreeSet::new();
        let mut current_chunks = BTreeSet::new();
        let mut inline = Vec::new();
        for segment in segments {
            let bytes = segment.as_bytes();
            if bytes.len() < CHUNK_MIN_BYTES {
                inline.extend_from_slice(bytes);
                continue;
            }
            flush_inline(&mut manifest, &mut inline)?;
            let key = (bytes.as_ptr() as usize, bytes.len());
            let digest = if let Some((_, digest)) = self.chunks.get(&key) { *digest } else {
                let digest: [u8; 32] = Sha256::digest(bytes).into();
                let path = self.chunk_path(&digest);
                match fs::read(&path) {
                    Ok(existing) if existing == bytes => {},
                    Ok(_) => return Err(invalid_manifest()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        let mut file = tempfile::NamedTempFile::new_in(&self.directory)?;
                        file.write_all(bytes)?;
                        file.as_file().sync_all()?;
                        file.persist(&path).map_err(|error| error.error)?;
                    }
                    Err(error) => return Err(error.into()),
                }
                self.chunks.insert(key, (segment.clone(), digest));
                digest
            };
            retained.insert(key);
            current_chunks.insert(digest);
            manifest.push(1);
            manifest.extend_from_slice(&u32::try_from(bytes.len()).map_err(|_| invalid_manifest())?.to_le_bytes());
            manifest.extend_from_slice(&digest);
        }
        flush_inline(&mut manifest, &mut inline)?;
        let digest = Sha256::digest(&manifest);
        manifest.extend_from_slice(&digest);
        // Chunk names must survive a crash before the manifest can refer to them.
        File::open(&self.directory)?.sync_all()?;
        self.atomic_write("world", &manifest)?;
        self.chunks.retain(|key, _| retained.contains(key));
        let previous = std::mem::replace(&mut self.current_chunks, current_chunks);
        for digest in previous.difference(&self.current_chunks) {
            match fs::remove_file(self.chunk_path(digest)) {
                Ok(()) => {},
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
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

const MANIFEST_MAGIC: &[u8; 4] = b"NWM1";
const CHUNK_MIN_BYTES: usize = 4096;

fn invalid_manifest() -> NorthError {
    NorthError::Configuration("Saved workspace data is incomplete or damaged; the files have been preserved".into())
}

fn take<'a>(bytes: &mut &'a [u8], count: usize) -> NorthResult<&'a [u8]> {
    let (value, rest) = bytes.split_at_checked(count).ok_or_else(invalid_manifest)?;
    *bytes = rest;
    Ok(value)
}

fn flush_inline(manifest: &mut Vec<u8>, inline: &mut Vec<u8>) -> NorthResult<()> {
    if inline.is_empty() { return Ok(()); }
    manifest.push(0);
    manifest.extend_from_slice(&u32::try_from(inline.len()).map_err(|_| invalid_manifest())?.to_le_bytes());
    manifest.append(inline);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_reuses_payload_files_and_reopens_exact_bytes() {
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let mut store = LocalStore::open(root.path(), cwd.path()).unwrap();
        let payload = AtomPayloadSegment::Text("history".repeat(100_000).into());
        let prefix = |value: &'static [u8]| AtomPayloadSegment::Bytes(value.into());
        let first = [prefix(b"first"), payload.clone()];
        store.checkpoint(&first).unwrap();
        let digest = Sha256::digest(payload.as_bytes()).into();
        let path = store.chunk_path(&digest);
        let inode = fs::metadata(&path).unwrap().ino();
        let manifest = store.directory.join("world");
        assert!(fs::metadata(&manifest).unwrap().len() < 1024);
        store.checkpoint(&[prefix(b"second"), payload.clone()]).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        let expected = [b"second".as_slice(), payload.as_bytes()].concat();
        assert_eq!(store.read().unwrap().unwrap(), expected);
        drop(store);
        let mut store = LocalStore::open(root.path(), cwd.path()).unwrap();
        assert_eq!(store.read().unwrap().unwrap(), expected);
        store.checkpoint(&[prefix(b"second"), payload.clone()]).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);

        let old_manifest = fs::read(&manifest).unwrap();
        let other = AtomPayloadSegment::Text("new value".repeat(100_000).into());
        let other_path = store.chunk_path(&Sha256::digest(other.as_bytes()).into());
        fs::create_dir(&other_path).unwrap();
        assert!(store.checkpoint(&[prefix(b"unpublished"), other]).is_err());
        assert_eq!(fs::read(&manifest).unwrap(), old_manifest);
        assert_eq!(store.read().unwrap().unwrap(), expected);

        fs::write(&path, b"corrupt").unwrap();
        assert!(store.read().is_err());
        assert_eq!(fs::read(&manifest).unwrap(), old_manifest);
    }

    #[test]
    fn published_manifest_reclaims_only_previously_referenced_chunks() {
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let mut store = LocalStore::open(root.path(), cwd.path()).unwrap();
        let old = AtomPayloadSegment::Text("old history".repeat(1000).into());
        let kept = AtomPayloadSegment::Text("kept history".repeat(1000).into());
        store.checkpoint(&[old.clone(), kept.clone()]).unwrap();
        let old_path = store.chunk_path(&Sha256::digest(old.as_bytes()).into());
        let kept_path = store.chunk_path(&Sha256::digest(kept.as_bytes()).into());
        drop(store);
        let mut store = LocalStore::open(root.path(), cwd.path()).unwrap();
        assert_eq!(store.read().unwrap().unwrap(), [old.as_bytes(), kept.as_bytes()].concat());
        store.checkpoint(&[kept.clone()]).unwrap();
        assert!(!old_path.exists());
        assert!(kept_path.exists());
        assert_eq!(store.read().unwrap().unwrap(), kept.as_bytes());
    }

    #[test]
    fn old_flat_world_is_read_before_first_segmented_checkpoint() {
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let mut store = LocalStore::open(root.path(), cwd.path()).unwrap();
        store.atomic_write("world", b"CWC1existing full checkpoint").unwrap();
        assert_eq!(store.read().unwrap().unwrap(), b"CWC1existing full checkpoint");
        store.checkpoint(&[AtomPayloadSegment::Bytes(b"CWC1new complete checkpoint".as_slice().into())]).unwrap();
        assert_eq!(store.read().unwrap().unwrap(), b"CWC1new complete checkpoint");
        let path = store.directory.join("world");
        let mut manifest = fs::read(&path).unwrap();
        manifest[12] ^= 1;
        fs::write(path, manifest).unwrap();
        assert!(store.read().is_err());
    }
}
