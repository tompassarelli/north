use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::platform::process;

use super::archive_extension;

pub(super) const DEPTH_CAP: usize = 8;
pub(super) const BYTE_CAP: u64 = 256 << 20;

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Member {
    pub(super) path: Vec<u8>,
    pub(super) depth: usize,
    pub(super) content: Vec<u8>,
}

pub(super) struct Unpacker {
    depth_cap: usize,
    byte_cap: u64,
    spent: u64,
}

fn lossy(path: &[u8]) -> String {
    String::from_utf8_lossy(path).into_owned()
}

fn escapes(name: &[u8]) -> bool {
    matches!(name.first(), Some(b'/' | b'\\')) || name.split(|byte| matches!(byte, b'/' | b'\\')).any(|part| part == b"..")
}

fn stem(path: &[u8]) -> Vec<u8> {
    let name = path.rsplit(|byte| *byte == b'/').next().unwrap_or(path);
    let renamed = [(".tgz", ".tar"), (".tbz2", ".tar"), (".txz", ".tar"), (".tzst", ".tar")]
        .iter()
        .find_map(|(from, to)| name.strip_suffix(from.as_bytes()).map(|base| [base, to.as_bytes()].concat()));
    let stripped = [".gz", ".bz2", ".xz", ".zst"].iter().find_map(|ext| name.strip_suffix(ext.as_bytes())).map(<[u8]>::to_vec);
    renamed.or(stripped).filter(|stem| !stem.is_empty()).unwrap_or_else(|| name.to_vec())
}

fn tool(program: &str, dir: &Path, args: &[&str], limit: u64) -> Result<process::Bounded, String> {
    let output = process::run_bounded(program, Some(dir), args, limit).map_err(|e| format!("cannot run {program}: {e}"))?;
    if output.status.success() || output.overflowed {
        return Ok(output);
    }
    let said = if output.stderr.is_empty() { &output.stdout } else { &output.stderr };
    Err(format!("{program} failed (exit {}): {}", output.status.code().unwrap_or(-1), String::from_utf8_lossy(said).trim()))
}

impl Unpacker {
    pub(super) fn new(depth_cap: usize, byte_cap: u64) -> Self {
        Self { depth_cap, byte_cap, spent: 0 }
    }

    fn spend(&mut self, path: &[u8], bytes: u64) -> Result<(), String> {
        self.spent = self.spent.saturating_add(bytes);
        if self.spent > self.byte_cap {
            return Err(format!("unpacking {} passes the {}-byte archive cap", lossy(path), self.byte_cap));
        }
        Ok(())
    }

    pub(super) fn members(&mut self, path: &[u8], content: &[u8], depth: usize, into: &mut Vec<Member>) -> Result<(), String> {
        let Some(ext) = archive_extension(content) else { return Ok(()) };
        if depth >= self.depth_cap {
            return Err(format!("{} is nested past the archive depth cap of {}", lossy(path), self.depth_cap));
        }
        let scratch = tempfile::tempdir().map_err(|e| format!("cannot create an unpack directory: {e}"))?;
        let (input, out) = (scratch.path().join("input"), scratch.path().join("out"));
        std::fs::write(&input, content)
            .and_then(|()| std::fs::create_dir(&out))
            .map_err(|e| format!("cannot stage {} for unpacking: {e}", lossy(path)))?;
        match ext {
            "zip" => self.extract_zip(path, scratch.path(), &out)?,
            "tar" => self.extract_tar(path, scratch.path(), &out)?,
            "gz" | "bz2" | "xz" | "zst" => return self.decompress(path, ext, scratch.path(), depth, into),
            _ => return Err(format!("cannot open the {ext} archive {}", lossy(path))),
        }
        self.walk(path, &out, &out, depth + 1, into)
    }

    fn check_names(path: &[u8], listing: &[u8]) -> Result<(), String> {
        let mut seen = std::collections::BTreeSet::new();
        for name in listing.split(|byte| *byte == b'\n').filter(|name| !name.is_empty()) {
            if escapes(name) {
                return Err(format!("{} holds a member that escapes the staging directory: {}", lossy(path), lossy(name)));
            }
            if name.ends_with(b"/") {
                continue;
            }
            let file = name.split(|byte| *byte == b'/').filter(|part| !part.is_empty() && *part != b".").collect::<Vec<_>>().join(&b'/');
            if !seen.insert(file.clone()) {
                return Err(format!("{} holds the member {} more than once", lossy(path), lossy(&file)));
            }
        }
        Ok(())
    }

    fn extract_zip(&mut self, path: &[u8], dir: &Path, out: &Path) -> Result<(), String> {
        let listing = tool("unzip", dir, &["-Z1", "input"], self.byte_cap)?;
        Self::check_names(path, &listing.stdout)?;
        let inflated = tool("unzip", dir, &["-p", "-P", "", "input"], self.byte_cap.saturating_sub(self.spent))?;
        if inflated.overflowed {
            return Err(format!("unpacking {} passes the {}-byte archive cap", lossy(path), self.byte_cap));
        }
        tool("unzip", dir, &["-qq", "-o", "-P", "", "input", "-d", &out.to_string_lossy()], self.byte_cap)?;
        Self::open_up(dir)
    }

    fn open_up(dir: &Path) -> Result<(), String> {
        tool("chmod", dir, &["-R", "u+rwX", "out"], 4096).map(drop)
    }

    fn extract_tar(&mut self, path: &[u8], dir: &Path, out: &Path) -> Result<(), String> {
        let listing = tool("tar", dir, &["-tf", "input"], self.byte_cap)?;
        Self::check_names(path, &listing.stdout)?;
        let out = out.to_string_lossy();
        tool("tar", dir, &["-xf", "input", "-C", &out, "--no-same-owner", "--no-same-permissions"], self.byte_cap)?;
        Self::open_up(dir)
    }

    fn decompress(&mut self, path: &[u8], ext: &str, dir: &Path, depth: usize, into: &mut Vec<Member>) -> Result<(), String> {
        let program = match ext {
            "gz" => "gzip",
            "bz2" => "bzip2",
            "xz" => "xz",
            _ => "zstd",
        };
        let output = tool(program, dir, &["-dcq", "input"], self.byte_cap.saturating_sub(self.spent))?;
        if output.overflowed {
            return Err(format!("unpacking {} passes the {}-byte archive cap", lossy(path), self.byte_cap));
        }
        self.spend(path, output.stdout.len() as u64)?;
        let member = [path, b"/", &stem(path)].concat();
        self.members(&member, &output.stdout, depth + 1, into)?;
        into.push(Member { path: member, depth: depth + 1, content: output.stdout });
        Ok(())
    }

    fn walk(&mut self, path: &[u8], out: &Path, dir: &Path, depth: usize, into: &mut Vec<Member>) -> Result<(), String> {
        let entries = std::fs::read_dir(dir).and_then(Iterator::collect::<std::io::Result<Vec<_>>>);
        let mut entries = entries.map_err(|e| format!("cannot read the members of {}: {e}", lossy(path)))?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let file = entry.path();
            let relative = file.strip_prefix(out).map_err(|_| format!("{} unpacked outside its staging directory", lossy(path)))?;
            let member = [path, b"/", relative.as_os_str().as_bytes()].concat();
            let kind = entry.file_type().map_err(|e| format!("cannot inspect {}: {e}", lossy(&member)))?;
            if kind.is_dir() {
                self.walk(path, out, &file, depth, into)?;
                continue;
            }
            let content = if kind.is_file() {
                self.spend(&member, entry.metadata().map_or(u64::MAX, |meta| meta.len()))?;
                std::fs::read(&file)
            } else if kind.is_symlink() {
                std::fs::read_link(&file).map(|target| target.as_os_str().as_bytes().to_vec())
            } else {
                return Err(format!("{} is neither a file, a directory nor a link", lossy(&member)));
            };
            let content = content.map_err(|e| format!("cannot read {}: {e}", lossy(&member)))?;
            if kind.is_file() {
                self.members(&member, &content, depth, into)?;
            }
            into.push(Member { path: member, depth, content });
        }
        Ok(())
    }
}
