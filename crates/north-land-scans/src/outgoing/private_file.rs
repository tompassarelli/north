use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use crate::landing::step::{Refused, Step};

pub const PREFIX: &str = "private-file: ";
const APPROVED: &str = "agents/approved-moves.toml";
const EMPTY_BLOB: &str = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";

#[derive(Debug, Eq, PartialEq)]
pub struct Hit {
    pub commit: String,
    pub path: String,
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git").current_dir(dir).args(["-c", "core.quotePath=false"]).args(args).output().map_err(|error| format!("git {args:?}: {error}"))?;
    if !output.status.success() {
        return Err(format!("git {args:?} in {}: {}", dir.display(), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn cache_dir() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from);
    state.unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")).join("north/private-blobs")
}

fn read_blobs(operator: &Path) -> Result<BTreeSet<String>, String> {
    let listed = git(operator, &["rev-list", "--objects", "--all"])?;
    let wanted: String = listed.lines().filter_map(|line| line.split(' ').next()).filter(|oid| !oid.is_empty()).map(|oid| format!("{oid}\n")).collect();
    let mut child = Command::new("git")
        .current_dir(operator)
        .args(["cat-file", "--batch-check=%(objecttype) %(objectname)"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("git cat-file: {error}"))?;
    let mut stdin = child.stdin.take().ok_or("git cat-file has no stdin")?;
    let writer = std::thread::spawn(move || stdin.write_all(wanted.as_bytes()));
    let output = child.wait_with_output().map_err(|error| format!("git cat-file: {error}"))?;
    writer.join().map_err(|_| "git cat-file writer panicked".to_owned())?.map_err(|error| format!("git cat-file: {error}"))?;
    if !output.status.success() {
        return Err(format!("git cat-file failed in {}", operator.display()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| line.strip_prefix("blob ")).filter(|oid| *oid != EMPTY_BLOB).map(str::to_owned).collect())
}

/// Every non-empty blob anywhere in the operator repository's history, cached under a hash of its HEAD and refs with the blob count in the name; a cache file that disagrees with its name or is empty is recomputed.
pub fn private_blobs(operator: &Path, cache: &Path) -> Result<BTreeSet<String>, String> {
    let refs = format!("{}{}", git(operator, &["rev-parse", "HEAD"])?, git(operator, &["for-each-ref", "--format=%(objectname) %(refname)"])?);
    let key: String = Sha256::digest(refs.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
    let read = |file: &Path| {
        let text = std::fs::read_to_string(file).ok()?;
        let blobs: BTreeSet<String> = text.lines().map(str::to_owned).collect();
        let name = file.file_name()?.to_str()?;
        (!blobs.is_empty() && name == format!("{key}-{}.txt", blobs.len())).then_some(blobs)
    };
    let cached = std::fs::read_dir(cache).ok().into_iter().flatten().filter_map(Result::ok).filter(|entry| entry.file_name().to_string_lossy().starts_with(&key)).find_map(|entry| read(&entry.path()));
    if let Some(blobs) = cached {
        return Ok(blobs);
    }
    let blobs = read_blobs(operator)?;
    if blobs.is_empty() {
        return Err(format!("{} holds no blobs, so it cannot be an operator repository", operator.display()));
    }
    let _ = std::fs::create_dir_all(cache);
    let body: String = blobs.iter().map(|oid| format!("{oid}\n")).collect();
    let staged = cache.join(format!("{key}.{}.tmp", std::process::id()));
    if std::fs::write(&staged, body).is_ok() {
        let _ = std::fs::rename(&staged, cache.join(format!("{key}-{}.txt", blobs.len())));
    }
    Ok(blobs)
}

/// The `path blob-oid` pairs the operator's landed approved-moves list names; a missing list names none.
pub fn approved(operator: &Path) -> Result<BTreeSet<(String, String)>, String> {
    let reference = if git(operator, &["rev-parse", "--verify", "-q", "origin/main"]).is_ok() { "origin/main" } else { "HEAD" };
    let Ok(text) = git(operator, &["show", &format!("{reference}:{APPROVED}")]) else { return Ok(BTreeSet::new()) };
    let document = text.parse::<toml_edit::DocumentMut>().map_err(|error| format!("{APPROVED}: {error}"))?;
    let moves = document.get("moves").and_then(|moves| moves.as_array()).ok_or_else(|| format!("{APPROVED} has no `moves` array of \"PATH BLOB-OID\" strings"))?;
    Ok(moves.iter().filter_map(|entry| entry.as_str()).filter_map(|entry| entry.rsplit_once(' ')).map(|(path, blob)| (path.to_owned(), blob.to_owned())).collect())
}

fn added(root: &Path, range: &str) -> Result<Vec<(String, String, String)>, String> {
    let log = git(root, &["log", "--raw", "-r", "--no-renames", "--no-abbrev", "--diff-filter=AM", "-m", "--format=%x01%h", range, "--"])?;
    let mut commit = String::new();
    let mut found = Vec::new();
    for line in log.lines() {
        if let Some(short) = line.strip_prefix('\x01') {
            short.clone_into(&mut commit);
        } else if let Some((meta, path)) = line.strip_prefix(':').and_then(|rest| rest.split_once('\t')) {
            let fields: Vec<&str> = meta.split(' ').collect();
            if let [_, mode, _, blob, _] = fields.as_slice()
                && *mode != "160000"
            {
                found.push((commit.clone(), path.to_owned(), (*blob).to_owned()));
            }
        }
    }
    Ok(found)
}

/// The files added in `range` whose blob is byte for byte a blob of the operator repository's history, unless `base` already held it or the operator's approved list names the path.
pub fn hits(root: &Path, operator: &Path, cache: &Path, range: &str, base: &str) -> Result<Vec<Hit>, String> {
    let private = private_blobs(operator, cache)?;
    let mut matched: Vec<(String, String, String)> = added(root, range)?.into_iter().filter(|(_, _, blob)| private.contains(blob)).collect();
    if matched.is_empty() {
        return Ok(Vec::new());
    }
    if !base.is_empty() {
        let held: BTreeSet<String> = git(root, &["ls-tree", "-r", base])?.lines().filter_map(|line| line.split_whitespace().nth(2)).map(str::to_owned).collect();
        matched.retain(|(_, _, blob)| !held.contains(blob));
    }
    let approved = approved(operator)?;
    let mut seen = BTreeSet::new();
    Ok(matched.into_iter().filter(|(_, path, blob)| !approved.contains(&(path.clone(), blob.clone()))).filter(|(commit, path, _)| seen.insert((commit.clone(), path.clone()))).map(|(commit, path, _)| Hit { commit, path }).collect())
}

pub fn outgoing(root: &Path, operator: &Path, cache: &Path, (base, head): (&str, &str)) -> Result<BTreeSet<String>, String> {
    if git(operator, &["rev-parse", "--git-dir"]).is_err() {
        return Ok(BTreeSet::new());
    }
    let range = if base.is_empty() { head.to_owned() } else { format!("{base}..{head}") };
    Ok(hits(root, operator, cache, &range, base)?.into_iter().map(|hit| format!("{PREFIX}{}", hit.path)).collect())
}

pub fn range_scan(root: &Path, operator: &Path, range: &str) -> Step {
    match hits(root, operator, &cache_dir(), range, "") {
        Ok(found) => {
            found.iter().for_each(|hit| println!("{PREFIX}{} {}", hit.commit, hit.path));
            if found.is_empty() { Ok(()) } else { Err(Refused(1)) }
        }
        Err(error) => {
            eprintln!("private-file: cannot scan: {error}");
            Err(Refused(2))
        }
    }
}
