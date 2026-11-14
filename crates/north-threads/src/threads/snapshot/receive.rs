//! The page stream a caller receives: it rebuilds the image in a temporary file, resumes a first pull
//! from the partial it kept, and publishes only a file that matches the sender's digest.
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use rustix::fs::{AtFlags, Mode, OFlags, CWD};

use super::pages::{Hash, MAX_PAGES, MAGIC, hash_of, invalid, list_digest, read_u32};
use super::sidecar::suffixed;

pub(super) fn receive(stdout: impl Read, directory: &Path, partial: Option<PathBuf>, old: Option<File>, old_size: usize, mut hashes: Vec<Hash>) -> io::Result<File> {
    let mut input = BufReader::with_capacity(1 << 16, stdout);
    let mut magic = [0; MAGIC.len()];
    input.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(invalid("the ledger host did not answer with a page stream"));
    }
    let size = read_u32(&mut input)? as usize;
    let count = read_u32(&mut input)?;
    if !(512..=65536).contains(&size) || !size.is_power_of_two() || count == 0 || count > MAX_PAGES {
        return Err(invalid("the page stream header is out of range"));
    }
    let file = File::from(rustix::fs::open(directory, OFlags::TMPFILE | OFlags::RDWR | OFlags::CLOEXEC, Mode::from_raw_mode(0o644))?);
    match old.filter(|_| old_size == size && !hashes.is_empty()) {
        Some(old) => clone_into(&old, &file)?,
        None => hashes.clear(),
    }
    hashes.resize(count as usize, [0; 32]);
    let finished = receive_pages(&mut input, &file, size, &mut hashes);
    if let (Err(_), Some(partial)) = (&finished, &partial) {
        let _ = keep_partial(&file, partial);
    }
    finished?;
    Ok(file)
}

/// The pages received so far stay in `partial`, so a first pull too big for one try resumes where it stopped.
fn keep_partial(file: &File, partial: &Path) -> io::Result<()> {
    let tmp = suffixed(partial, ".tmp");
    let _ = fs::remove_file(&tmp);
    rustix::fs::linkat(CWD, format!("/proc/self/fd/{}", file.as_raw_fd()), CWD, &tmp, AtFlags::SYMLINK_FOLLOW)?;
    fs::rename(&tmp, partial)
}

fn receive_pages(input: &mut impl Read, file: &File, size: usize, hashes: &mut [Hash]) -> io::Result<()> {
    let mut page = vec![0; size];
    loop {
        let number = read_u32(input)?;
        if number == 0 {
            break;
        }
        if number as usize > hashes.len() {
            return Err(invalid("the page stream names a page beyond the image"));
        }
        input.read_exact(&mut page)?;
        file.write_all_at(&page, u64::from(number - 1) * size as u64)?;
        hashes[number as usize - 1] = hash_of(&page);
    }
    let mut digest = [0; 32];
    input.read_exact(&mut digest)?;
    if input.read(&mut [0])? != 0 {
        return Err(invalid("the page stream has trailing bytes"));
    }
    file.set_len(hashes.len() as u64 * size as u64)?;
    if list_digest(hashes) != digest {
        return Err(invalid("the rebuilt snapshot does not match the ledger host's digest"));
    }
    Ok(())
}

fn clone_into(old: &File, file: &File) -> io::Result<()> {
    if rustix::fs::ioctl_ficlone(file, old).is_ok() {
        return Ok(());
    }
    let length = old.metadata()?.len();
    let (mut from, mut to) = (0u64, 0u64);
    while from < length {
        let remaining = (length - from) as usize;
        let copied = rustix::fs::copy_file_range(old, Some(&mut from), file, Some(&mut to), remaining)?;
        if copied == 0 {
            break;
        }
    }
    Ok(())
}

pub(super) fn publish(file: &File, snapshot: &Path) -> io::Result<()> {
    let tmp = suffixed(snapshot, &format!(".{}.tmp", std::process::id()));
    let _ = fs::remove_file(&tmp);
    rustix::fs::linkat(CWD, format!("/proc/self/fd/{}", file.as_raw_fd()), CWD, &tmp, AtFlags::SYMLINK_FOLLOW)?;
    if let Err(error) = fs::rename(&tmp, snapshot) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    sweep(snapshot);
    Ok(())
}

fn sweep(snapshot: &Path) {
    let (Some(directory), Some(name)) = (snapshot.parent(), snapshot.file_name().and_then(|name| name.to_str())) else {
        return;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let file = entry.file_name();
        let Some(pid) = file.to_str().and_then(|file| file.strip_prefix(name)).and_then(|rest| rest.strip_prefix('.'))
            .and_then(|rest| rest.strip_suffix(".tmp")).filter(|pid| !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()))
        else {
            continue;
        };
        if !Path::new("/proc").join(pid).exists() {
            let _ = fs::remove_file(entry.path());
        }
    }
}
