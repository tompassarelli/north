use std::ffi::OsStr;
use std::io::{self, Error, ErrorKind};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

const BLOCK: usize = 512;

pub(super) fn pack(parent: &Path, directory: &OsStr) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    add(&mut out, parent, Path::new(directory))?;
    out.resize(out.len() + 2 * BLOCK, 0);
    Ok(out)
}

fn add(out: &mut Vec<u8>, parent: &Path, relative: &Path) -> io::Result<()> {
    let path = parent.join(relative);
    let meta = std::fs::symlink_metadata(&path)?;
    let name = relative.to_str().ok_or_else(|| invalid(&path, "is not UTF-8"))?;
    let (mode, mtime) = (meta.mode() & 0o7777, meta.mtime().max(0) as u64);
    if meta.is_dir() {
        out.extend_from_slice(&header(&format!("{name}/"), mode, 0, mtime, b'5', "")?);
        let mut entries = std::fs::read_dir(&path)?.map(|entry| entry.map(|entry| entry.file_name())).collect::<io::Result<Vec<_>>>()?;
        entries.sort();
        for entry in entries {
            add(out, parent, &relative.join(entry))?;
        }
    } else if meta.is_symlink() {
        let target = std::fs::read_link(&path)?;
        out.extend_from_slice(&header(name, mode, 0, mtime, b'2', target.to_str().ok_or_else(|| invalid(&path, "links to a non-UTF-8 target"))?)?);
    } else if meta.is_file() {
        let content = std::fs::read(&path)?;
        out.extend_from_slice(&header(name, mode, content.len() as u64, mtime, b'0', "")?);
        out.extend_from_slice(&content);
        out.resize(out.len().next_multiple_of(BLOCK), 0);
    } else {
        return Err(invalid(&path, "is neither a file, a directory nor a symlink"));
    }
    Ok(())
}

fn header(name: &str, mode: u32, size: u64, mtime: u64, kind: u8, link: &str) -> io::Result<[u8; BLOCK]> {
    let split = split_name(name).ok_or_else(|| invalid(Path::new(name), "is too long for a ustar archive"))?;
    if link.len() > 100 || size >= 1 << 33 {
        return Err(invalid(Path::new(name), "is too large for a ustar archive"));
    }
    let mut block = [0u8; BLOCK];
    // ustar layout: name 0..100, mode 100, uid 108, gid 116, size 124, mtime 136, checksum 148, type 156, link 157, magic 257, version 263, prefix 345
    put(&mut block, 0, 100, split.1.as_bytes());
    put(&mut block, 100, 8, octal(u64::from(mode), 8).as_bytes());
    put(&mut block, 108, 8, octal(0, 8).as_bytes());
    put(&mut block, 116, 8, octal(0, 8).as_bytes());
    put(&mut block, 124, 12, octal(size, 12).as_bytes());
    put(&mut block, 136, 12, octal(mtime, 12).as_bytes());
    block[156] = kind;
    put(&mut block, 157, 100, link.as_bytes());
    put(&mut block, 257, 6, b"ustar\0");
    put(&mut block, 263, 2, b"00");
    put(&mut block, 345, 155, split.0.as_bytes());
    block[148..156].fill(b' ');
    let sum: u32 = block.iter().map(|byte| u32::from(*byte)).sum();
    put(&mut block, 148, 8, format!("{sum:06o}\0 ").as_bytes());
    Ok(block)
}

fn split_name(name: &str) -> Option<(&str, &str)> {
    if name.len() <= 100 {
        return Some(("", name));
    }
    name.match_indices('/').map(|(at, _)| (&name[..at], &name[at + 1..])).find(|(prefix, rest)| prefix.len() <= 155 && !rest.is_empty() && rest.len() <= 100)
}

fn octal(value: u64, width: usize) -> String {
    format!("{value:0digits$o}\0", digits = width - 1)
}

fn put(block: &mut [u8; BLOCK], at: usize, width: usize, bytes: &[u8]) {
    let length = bytes.len().min(width);
    block[at..at + length].copy_from_slice(&bytes[..length]);
}

fn invalid(path: &Path, why: &str) -> Error {
    Error::new(ErrorKind::InvalidInput, format!("{} {why}", path.display()))
}

#[cfg(test)]
#[path = "archive_tests.rs"]
mod tests;
