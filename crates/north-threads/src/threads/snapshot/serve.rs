//! The ledger host's side of a pull: it keeps one built image beside the ledger and streams the pages a
//! caller's manifest lacks, from that image while the ledger has not been written since it was built.
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::image::image;
use super::pages::{Hash, MAX_PAGES, MAGIC, be32, hash_of, invalid, page_size, read_manifest};
use super::sidecar::suffixed;

pub const REMOTE_COMMAND: &str = "north threads snapshot-pages";
/// How long the ledger host serves one built image of a changed ledger to callers that accept a recent one, so the peers' pulls, which come about six minutes apart each, share one build of the 800 MB ledger.
const REUSE_SECONDS: u64 = 600;
const STAMP_MARGIN: Duration = Duration::from_millis(50);

/// Streams the pages of the ledger image that differ from the caller's manifest; `withhold_native`
/// builds the image with every row naming a native item removed. A manifest that accepts a recent image
/// is answered from the image the last build left beside the ledger, built anew once that is older than `REUSE_SECONDS`; any manifest is
/// answered from it while the ledger has not been written since that build began.
pub fn serve(database: &Path, withhold_native: bool, input: &mut dyn Read, output: &mut dyn Write) -> io::Result<()> {
    let (manifest_size, manifest, recent) = read_manifest(input)?;
    let served = served(database, withhold_native, if recent { Duration::from_secs(REUSE_SECONDS) } else { Duration::ZERO })?;
    let size = served.size;
    let mut out = BufWriter::with_capacity(1 << 16, output);
    out.write_all(MAGIC)?;
    out.write_all(&(size as u32).to_be_bytes())?;
    out.write_all(&(served.hashes.len() as u32).to_be_bytes())?;
    let mut page = vec![0; size];
    for (index, hash) in served.hashes.iter().enumerate() {
        if !(manifest_size == size && manifest.get(index) == Some(hash)) {
            served.file.read_exact_at(&mut page, served.body + (index * size) as u64)?;
            out.write_all(&(index as u32 + 1).to_be_bytes())?;
            out.write_all(&page)?;
        }
    }
    out.write_all(&0u32.to_be_bytes())?;
    out.write_all(&served.digest)?;
    out.flush()
}

/// A built image kept beside the ledger: its page size, page count, the hash of every page, the hash of the whole image, then the image itself from `body`.
struct Served {
    file: File,
    size: usize,
    hashes: Vec<Hash>,
    digest: Hash,
    body: u64,
}

fn served(database: &Path, withhold_native: bool, reuse: Duration) -> io::Result<Served> {
    let path = suffixed(database, if withhold_native { ".served-withheld" } else { ".served-full" });
    if let Some(found) = open_served(&path, database, reuse) {
        return Ok(found);
    }
    let building = OpenOptions::new().create(true).append(true).open(suffixed(&path, ".lock"))?;
    building.lock()?;
    if let Some(found) = open_served(&path, database, reuse) {
        return Ok(found);
    }
    let stamp = SystemTime::now() - STAMP_MARGIN;
    let image = image(database, withhold_native)?;
    let size = page_size(&image).ok_or_else(|| invalid("the ledger image has no SQLite header"))?;
    let hashes: Vec<Hash> = image.chunks_exact(size).map(hash_of).collect();
    let mut head = (size as u32).to_be_bytes().to_vec();
    head.extend((hashes.len() as u32).to_be_bytes());
    hashes.iter().for_each(|hash| head.extend(hash));
    head.extend(hash_of(&image));
    let tmp = suffixed(&path, &format!(".{}.tmp", std::process::id()));
    let written = File::create(&tmp).and_then(|mut file| file.write_all(&head).and_then(|()| file.write_all(&image)).and_then(|()| file.set_modified(stamp)));
    if let Err(error) = written.and_then(|()| fs::rename(&tmp, &path)) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    open_served(&path, database, Duration::MAX).ok_or_else(|| invalid("the built ledger image cannot be read back"))
}

fn last_written(database: &Path) -> Option<SystemTime> {
    let modified = |path: &Path| fs::metadata(path).and_then(|meta| meta.modified());
    let log = modified(&suffixed(database, "-wal")).ok();
    Some(modified(database).ok()?.max(log.unwrap_or(SystemTime::UNIX_EPOCH)))
}

fn open_served(path: &Path, database: &Path, within: Duration) -> Option<Served> {
    let file = File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    let built = meta.modified().ok()?;
    let unchanged = last_written(database).is_some_and(|written| written <= built);
    if !unchanged && SystemTime::now().duration_since(built).is_ok_and(|age| age > within) {
        return None;
    }
    let mut head = [0; 8];
    file.read_exact_at(&mut head, 0).ok()?;
    let (size, count) = (be32(&head) as usize, be32(&head[4..]) as usize);
    let listed = 8 + 32 * (count + 1);
    if !(512..=65536).contains(&size) || count == 0 || count > MAX_PAGES as usize || meta.len() != (listed + size * count) as u64 {
        return None;
    }
    let mut listing = vec![0; 32 * (count + 1)];
    file.read_exact_at(&mut listing, 8).ok()?;
    let mut hashes: Vec<Hash> = listing.as_chunks::<32>().0.to_vec();
    let digest = hashes.pop()?;
    Some(Served { file, size, hashes, digest, body: listed as u64 })
}
