use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::{NorthError, NorthResult};

pub const DIR: &str = ".attachments";
const OPEN: &str = "[image: ";
const TYPES: &[(&str, &str)] = &[("image/png", "png"), ("image/jpeg", "jpg"), ("image/gif", "gif"), ("image/webp", "webp")];

pub fn marker(path: &Path) -> String {
    format!("{OPEN}{}]", path.display())
}

pub fn extension(media_type: &str) -> Option<&'static str> {
    TYPES.iter().find(|(known, _)| *known == media_type).map(|(_, extension)| *extension)
}

pub fn media_type(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let extension = if extension == "jpeg" { "jpg" } else { extension.as_str() };
    TYPES.iter().find(|(_, known)| *known == extension).map(|(media_type, _)| *media_type)
}

// Only files inside a .attachments directory are read, so a message cannot name an arbitrary file to embed.
fn owned(path: &Path) -> bool {
    path.is_absolute()
        && path.components().all(|part| part != Component::ParentDir)
        && path.parent().and_then(Path::file_name) == Some(OsStr::new(DIR))
        && media_type(path).is_some()
}

pub fn paths(text: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some((_, tail)) = rest.split_once(OPEN) {
        let Some((path, after)) = tail.split_once(']') else { break };
        let path = Path::new(path);
        if owned(path) {
            found.push(path.to_path_buf());
        }
        rest = after;
    }
    found
}

pub fn read(text: &str) -> NorthResult<Vec<(&'static str, Vec<u8>)>> {
    paths(text)
        .into_iter()
        .filter_map(|path| Some((media_type(&path)?, path)))
        .map(|(media_type, path)| match fs::read(&path) {
            Ok(bytes) => Ok((media_type, bytes)),
            Err(error) => Err(NorthError::Usage(format!("cannot read attachment {}: {error}", path.display()))),
        })
        .collect()
}

pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = u32::from(chunk[0]) << 16 | u32::from(*chunk.get(1).unwrap_or(&0)) << 8 | u32::from(*chunk.get(2).unwrap_or(&0));
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                out.push(char::from(ALPHABET[(group >> shift & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_pads_like_the_standard_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
