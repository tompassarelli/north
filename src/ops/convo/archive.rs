use std::collections::HashSet;
use std::fs::{self, File, FileTimes};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use super::discover::Stat;
use super::env::Env;
use super::failure::{Failure, Result};

pub(super) const ZSTD_ARGS: [&str; 2] = ["-3", "--long=27"];
const ROSTER_WAIT: Duration = Duration::from_secs(180);

fn unsafe_to_sweep(message: String) -> Failure {
    Failure::Unsafe(message)
}

pub(super) struct OpenFiles {
    pub paths: HashSet<String>,
    pub unseen: Vec<String>,
    pub others: usize,
}

fn comm(pid: &str) -> String {
    fs::read_to_string(format!("/proc/{pid}/comm")).ok().map(|name| name.trim().to_owned()).filter(|name| !name.is_empty()).unwrap_or_else(|| pid.to_owned())
}

fn owner(pid: &str) -> Option<u32> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status.lines().find_map(|line| line.strip_prefix("Uid:")?.split_whitespace().next()?.parse().ok())
}

/// Transcripts any process holds open, read from /proc; `unseen` names this user's processes whose fd tables no tool can read.
pub(super) fn open_files() -> OpenFiles {
    let mut open = OpenFiles { paths: HashSet::new(), unseen: Vec::new(), others: 0 };
    let me = rustix::process::getuid().as_raw();
    let Ok(entries) = fs::read_dir("/proc") else { return open };
    for entry in entries.flatten() {
        let pid = entry.file_name().to_string_lossy().into_owned();
        if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let dir = format!("/proc/{pid}/fd");
        let names = match fs::read_dir(&dir) {
            Ok(names) => names,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                if owner(&pid) == Some(me) {
                    open.unseen.push(comm(&pid));
                } else {
                    open.others += 1;
                }
                continue;
            }
        };
        for name in names.flatten() {
            let Ok(target) = fs::read_link(name.path()) else { continue };
            let text = target.to_string_lossy();
            let text = text.strip_suffix(" (deleted)").unwrap_or(&text);
            if text.starts_with('/') && (text.ends_with(".jsonl") || text.ends_with(".jsonl.zst")) {
                open.paths.insert(text.to_owned());
            }
        }
    }
    open
}

pub(super) fn shell_split(text: &str) -> Option<Vec<String>> {
    let (mut words, mut word, mut started) = (Vec::new(), String::new(), false);
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        match character {
            c if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            '\\' => {
                word.push(chars.next()?);
                started = true;
            }
            quote @ ('\'' | '"') => {
                started = true;
                loop {
                    match chars.next()? {
                        c if c == quote => break,
                        '\\' if quote == '"' => word.push(chars.next()?),
                        c => word.push(c),
                    }
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    Some(words)
}

fn uuids(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let shape = [8, 4, 4, 4, 12];
    let mut found = Vec::new();
    let mut at = 0;
    while at + 36 <= bytes.len() {
        let window = &bytes[at..at + 36];
        let mut offset = 0;
        let valid = shape.iter().enumerate().all(|(index, length)| {
            let part = &window[offset..offset + length];
            let separator_ok = index == 0 || window[offset - 1] == b'-';
            offset += length + 1;
            separator_ok && part.iter().all(u8::is_ascii_hexdigit)
        });
        if valid {
            found.push(text[at..at + 36].to_lowercase());
            at += 36;
        } else {
            at += 1;
        }
    }
    found
}

fn collect_ids(value: &serde_json::Value, out: &mut HashSet<String>) {
    match value {
        serde_json::Value::String(text) => out.extend(uuids(text)),
        serde_json::Value::Object(map) => map.values().filter(|value| value.is_string()).for_each(|value| collect_ids(value, out)),
        _ => {}
    }
}

/// Session ids the coordinator accounts for; any failure aborts the sweep.
pub(super) fn coordinator_ids(env: &Env) -> Result<HashSet<String>> {
    let text = env.roster_cmd.clone().unwrap_or_else(|| "north agents --json".to_owned());
    let words = shell_split(&text).filter(|words| !words.is_empty()).ok_or_else(|| unsafe_to_sweep("the roster command is not readable".to_owned()))?;
    let mut child = Command::new(&words[0])
        .args(&words[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| unsafe_to_sweep(format!("could not ask the coordinator which sessions are live: {error}")))?;
    let (mut stdout, mut stderr) = (child.stdout.take(), child.stderr.take());
    let out = std::thread::spawn(move || stdout.as_mut().map(|pipe| { let mut text = String::new(); let _ = pipe.read_to_string(&mut text); text }).unwrap_or_default());
    let err = std::thread::spawn(move || stderr.as_mut().map(|pipe| { let mut text = String::new(); let _ = pipe.read_to_string(&mut text); text }).unwrap_or_default());
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > ROSTER_WAIT {
            let _ = child.kill();
            return Err(unsafe_to_sweep("could not ask the coordinator which sessions are live: timed out".to_owned()));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
    if !status.success() {
        let last = err.trim().lines().last().map_or_else(|| format!("exit {}", status.code().unwrap_or(-1)), str::to_owned);
        return Err(unsafe_to_sweep(format!("the coordinator refused to report live sessions: {last}")));
    }
    let roster: serde_json::Value =
        serde_json::from_str(&out).map_err(|error| unsafe_to_sweep(format!("the coordinator roster was not readable JSON: {error}")))?;
    let agents = roster.get("agents").and_then(serde_json::Value::as_array);
    if roster.get("version").and_then(serde_json::Value::as_str) != Some("north:agent-roster:v1") || agents.is_none() {
        return Err(unsafe_to_sweep("the coordinator roster is not north:agent-roster:v1".to_owned()));
    }
    let mut ids = HashSet::new();
    agents.into_iter().flatten().for_each(|row| collect_ids(row, &mut ids));
    Ok(ids)
}

pub(super) fn fsync_path(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}

fn round_trips(zstd: &str, archive: &Path, original: &Path) -> bool {
    let Ok(mut child) = Command::new(zstd).arg("-dcq").arg("--").arg(archive).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { return false };
    let (Some(mut decoded), Ok(mut source)) = (child.stdout.take(), File::open(original)) else { return false };
    let (mut left, mut right) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
    let same = loop {
        let count = match fill(&mut decoded, &mut left) {
            Ok(count) => count,
            Err(_) => break false,
        };
        match fill(&mut source, &mut right[..count.max(1)]) {
            Ok(got) if got == count && left[..count] == right[..count] => {}
            _ => break false,
        }
        if count == 0 {
            break true;
        }
    };
    if !same {
        let _ = child.kill();
    }
    child.wait().is_ok_and(|status| status.success()) && same
}

fn fill(reader: &mut impl Read, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0;
    while total < buffer.len() {
        let count = reader.read(&mut buffer[total..])?;
        if count == 0 {
            break;
        }
        total += count;
    }
    Ok(total)
}

fn timestamp(pair: (i64, i64)) -> SystemTime {
    let moment = Duration::new(pair.0.unsigned_abs(), pair.1 as u32);
    if pair.0 >= 0 { SystemTime::UNIX_EPOCH + moment } else { SystemTime::UNIX_EPOCH - moment }
}

fn copy_times(path: &Path, stat: &Stat) -> std::io::Result<()> {
    File::options().write(true).open(path)?.set_times(FileTimes::new().set_accessed(timestamp(stat.atime)).set_modified(timestamp(stat.mtime)))
}

fn unlink_quiet(path: &str) {
    let _ = fs::remove_file(path);
}

fn publish(env: &Env, path: &str, stat: &Stat, tmp: &str, dst: &str) -> Result<u64> {
    let source = Path::new(path);
    let out = Command::new(&env.zstd).args(ZSTD_ARGS).args(["-q", "-f", "-o", tmp, "--", path]).output()?;
    if !out.status.success() {
        return Err(unsafe_to_sweep(format!("zstd failed: {}", String::from_utf8_lossy(&out.stderr).trim())));
    }
    if !round_trips(&env.zstd, Path::new(tmp), source) {
        return Err(unsafe_to_sweep("round-trip did not reproduce the original".to_owned()));
    }
    let now = Stat::of(&fs::metadata(path)?);
    if (now.ino, now.dev, now.size, now.mtime) != (stat.ino, stat.dev, stat.size, stat.mtime) {
        return Err(unsafe_to_sweep("the transcript was written during compression".to_owned()));
    }
    fsync_path(Path::new(tmp))?;
    copy_times(Path::new(tmp), stat)?;
    fs::rename(tmp, dst)?;
    let dir = source.parent().unwrap_or(Path::new("."));
    fsync_path(dir)?;
    fs::remove_file(path)?;
    fsync_path(dir)?;
    Ok(fs::metadata(dst)?.len())
}

/// Writes, verifies, proves the source never moved, publishes, then unlinks.
pub(super) fn compress_one(env: &Env, path: &str, stat: &Stat) -> Result<u64> {
    let dst = format!("{path}.zst");
    let tmp = format!("{dst}.tmp{}", std::process::id());
    let result = if Path::new(&dst).exists() && round_trips(&env.zstd, Path::new(&dst), Path::new(path)) {
        fs::remove_file(path).map_err(Failure::from).and_then(|()| Ok(fs::metadata(&dst)?.len()))
    } else {
        publish(env, path, stat, &tmp, &dst)
    };
    unlink_quiet(&tmp);
    result
}

/// Turns `.jsonl.zst` back into `.jsonl`; zstd's frame checksum proves integrity.
pub(super) fn restore_one(env: &Env, name: &str, err: &mut dyn std::io::Write) -> Result<Option<String>> {
    let logical = name.strip_suffix(".zst").unwrap_or(name);
    let source = format!("{logical}.zst");
    if !Path::new(&source).exists() || Path::new(logical).exists() {
        let reason = if Path::new(&source).exists() { format!("{logical} already exists") } else { format!("no archive at {source}") };
        writeln!(err, "convo: {reason}")?;
        return Ok(None);
    }
    let tmp = format!("{logical}.tmp{}", std::process::id());
    let out = Command::new(&env.zstd).args(["-dq", "-f", "-o", &tmp, "--", &source]).output()?;
    if !out.status.success() {
        writeln!(err, "convo: {source}: {}", String::from_utf8_lossy(&out.stderr).trim())?;
        unlink_quiet(&tmp);
        return Ok(None);
    }
    let meta = fs::metadata(&source)?;
    copy_times(Path::new(&tmp), &Stat::of(&meta))?;
    fsync_path(Path::new(&tmp))?;
    fs::rename(&tmp, logical)?;
    let dir = Path::new(logical).parent().unwrap_or(Path::new("."));
    fsync_path(dir)?;
    fs::remove_file(&source)?;
    fsync_path(dir)?;
    unlink_quiet(&tmp);
    Ok(Some(format!("restored {logical} ({})", super::compress::gib(fs::metadata(logical)?.size()))))
}
