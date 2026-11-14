//! The page format both sides share: the manifest that names the pages a caller already has and the
//! hashes that let a receiver prove its rebuilt image matches the sender's. Every field is big-endian,
//! and a page hash is the SHA-256 of the page's bytes.
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::FileExt;

use sha2::{Digest, Sha256};

pub const MAGIC: &[u8] = b"NORTH-SNAPSHOT-PAGES 3\n";
/// Opens a manifest that accepts an image built up to `REUSE_SECONDS` ago; `MAGIC` opens one that demands an image of the ledger as it is now, which an unchanged ledger's kept image is.
const MAGIC_RECENT: &[u8] = b"NORTH-SNAPSHOT-PAGES 4\n";
const SQLITE_HEADER: &[u8] = b"SQLite format 3\0";
pub(super) const MAX_PAGES: u32 = 1 << 24;

pub(super) type Hash = [u8; 32];

pub fn manifest_bytes(size: usize, hashes: &[Hash], recent: bool) -> Vec<u8> {
    let mut bytes = if recent { MAGIC_RECENT } else { MAGIC }.to_vec();
    bytes.extend((size as u32).to_be_bytes());
    bytes.extend((hashes.len() as u32).to_be_bytes());
    hashes.iter().for_each(|hash| bytes.extend(hash));
    bytes
}

pub(super) fn read_manifest(input: &mut dyn Read) -> io::Result<(usize, Vec<Hash>, bool)> {
    let mut magic = [0; MAGIC.len()];
    input.read_exact(&mut magic)?;
    let recent = magic == MAGIC_RECENT;
    if magic != MAGIC && !recent {
        return Err(invalid("the page manifest has no magic line"));
    }
    let size = read_u32(input)? as usize;
    let count = read_u32(input)?;
    if count > MAX_PAGES {
        return Err(invalid("the page manifest is too long"));
    }
    let mut hashes = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let mut hash = [0; 32];
        input.read_exact(&mut hash)?;
        hashes.push(hash);
    }
    Ok((size, hashes, recent))
}

pub(super) fn hash_pages(file: &File) -> (usize, Vec<Hash>) {
    let mut header = [0; 100];
    if file.read_exact_at(&mut header, 0).is_err() {
        return (0, Vec::new());
    }
    let (Some(size), Ok(meta)) = (page_size(&header), file.metadata()) else {
        return (0, Vec::new());
    };
    let mut page = vec![0; size];
    let mut hashes = Vec::new();
    for index in 0..meta.len() / size as u64 {
        if file.read_exact_at(&mut page, index * size as u64).is_err() {
            return (0, Vec::new());
        }
        hashes.push(hash_of(&page));
    }
    (size, hashes)
}

/// The digest both sides compare: the SHA-256 of the image's page hashes in order, so a receiver proves its image from the hashes it already holds and the pages it wrote, without rereading the file.
pub(super) fn list_digest(hashes: &[Hash]) -> Hash {
    hashes.iter().fold(Sha256::new(), |hasher, hash| hasher.chain_update(hash)).finalize().into()
}

pub(super) fn page_size(header: &[u8]) -> Option<usize> {
    if header.len() < 100 || !header.starts_with(SQLITE_HEADER) {
        return None;
    }
    match be16(&header[16..]) {
        1 => Some(65536),
        size if size >= 512 && size.is_power_of_two() => Some(usize::from(size)),
        _ => None,
    }
}

pub(super) fn read_u32(input: &mut dyn Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}

pub(super) fn be16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

pub(super) fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

pub(super) fn hash_of(bytes: &[u8]) -> Hash {
    Sha256::digest(bytes).into()
}

/// The error every malformed manifest, page stream or image is refused with.
pub(super) fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}
