use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::ops::Range;
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};
use rustix::fs::{AtFlags, Mode, OFlags, CWD};
use sha2::{Digest, Sha256};

pub(crate) const MAGIC: &[u8] = b"NORTH-SNAPSHOT-PAGES 1\n";
pub(crate) const REMOTE_COMMAND: &str = "north threads snapshot-pages";
pub(crate) const SNAPSHOT_SECONDS: u64 = 900;
pub(crate) const PULL_TIMEOUT: Duration = Duration::from_secs(80);
const SQLITE_HEADER: &[u8] = b"SQLite format 3\0";
const MAX_PAGES: u32 = 1 << 24;

type Hash = [u8; 32];

pub(crate) fn serve(database: &Path, input: &mut dyn Read, output: &mut dyn Write) -> io::Result<()> {
    let (manifest_size, manifest) = read_manifest(input)?;
    let image = image(database)?;
    let size = page_size(&image).ok_or_else(|| invalid("the ledger image has no SQLite header"))?;
    let mut out = BufWriter::with_capacity(1 << 16, output);
    out.write_all(MAGIC)?;
    out.write_all(&(size as u32).to_be_bytes())?;
    out.write_all(&((image.len() / size) as u32).to_be_bytes())?;
    for (index, page) in image.chunks_exact(size).enumerate() {
        let unchanged = manifest_size == size && manifest.get(index).is_some_and(|hash| *hash == hash_of(page));
        if !unchanged {
            out.write_all(&(index as u32 + 1).to_be_bytes())?;
            out.write_all(page)?;
        }
    }
    out.write_all(&0u32.to_be_bytes())?;
    out.write_all(&hash_of(&image))?;
    out.flush()
}

pub(crate) fn refresh(ssh: &[OsString], snapshot: &Path, force: bool, wait: Duration) -> io::Result<bool> {
    let tried = suffixed(snapshot, ".tried");
    let newest = [snapshot, tried.as_path()].iter().filter_map(|path| fs::metadata(path).and_then(|meta| meta.modified()).ok()).max();
    if !force && newest.is_some_and(|modified| {
        SystemTime::now().duration_since(modified).map_or(true, |age| age.as_secs_f64() < SNAPSHOT_SECONDS as f64)
    }) {
        return Ok(false);
    }
    if let Some(parent) = snapshot.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new().create(true).append(true).open(&tried)?.set_modified(SystemTime::now())?;
    pull(ssh, snapshot, wait)?;
    Ok(true)
}

pub(crate) fn pull(ssh: &[OsString], snapshot: &Path, wait: Duration) -> io::Result<()> {
    let old = File::open(snapshot).ok();
    let (size, hashes) = old.as_ref().map(hash_pages).unwrap_or((0, Vec::new()));
    let directory = snapshot.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    let mut child = Command::new(&ssh[0]).args(&ssh[1..])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let manifest = manifest_bytes(size, &hashes);
    let mut stdin = child.stdin.take().ok_or_else(|| invalid("ssh has no stdin"))?;
    std::thread::spawn(move || {
        let _ = stdin.write_all(&manifest);
    });
    let stdout = child.stdout.take().ok_or_else(|| invalid("ssh has no stdout"))?;
    let receiver = std::thread::spawn(move || receive(stdout, &directory, old, size, hashes.len()));
    let deadline = Instant::now() + wait;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, "snapshot pull timed out"));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let built = receiver.join().map_err(|_| invalid("snapshot receiver panicked"))?;
    if !status.success() {
        return Err(io::Error::other(format!("ssh exited with {status}")));
    }
    let file = built?;
    file.sync_all()?;
    publish(&file, snapshot)
}

fn receive(stdout: impl Read, directory: &Path, old: Option<File>, old_size: usize, old_pages: usize) -> io::Result<File> {
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
    if let Some(old) = old.filter(|_| old_size == size && old_pages > 0) {
        clone_into(&old, &file)?;
    }
    let mut page = vec![0; size];
    loop {
        let number = read_u32(&mut input)?;
        if number == 0 {
            break;
        }
        if number > count {
            return Err(invalid("the page stream names a page beyond the image"));
        }
        input.read_exact(&mut page)?;
        file.write_all_at(&page, u64::from(number - 1) * size as u64)?;
    }
    let mut digest = [0; 32];
    input.read_exact(&mut digest)?;
    if input.read(&mut [0])? != 0 {
        return Err(invalid("the page stream has trailing bytes"));
    }
    file.set_len(u64::from(count) * size as u64)?;
    if hash_file(&file)? != digest {
        return Err(invalid("the rebuilt snapshot does not match the ledger host's digest"));
    }
    Ok(file)
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

fn publish(file: &File, snapshot: &Path) -> io::Result<()> {
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

fn image(database: &Path) -> io::Result<Vec<u8>> {
    let source = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(sql)?;
    source.busy_timeout(Duration::from_secs(10)).map_err(sql)?;
    let size: i64 = source.query_row("PRAGMA page_size", [], |row| row.get(0)).map_err(sql)?;
    let mut memory = Connection::open_in_memory().map_err(sql)?;
    memory.execute_batch(&format!("PRAGMA page_size={size}")).map_err(sql)?;
    {
        let backup = Backup::new(&source, &mut memory).map_err(sql)?;
        while backup.step(-1).map_err(sql)? != StepResult::Done {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let btree: Vec<u32> = memory.prepare("SELECT pageno FROM dbstat WHERE pagetype IN ('internal', 'leaf')").map_err(sql)?
        .query_map([], |row| row.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    let mut bytes = memory.serialize("main").map_err(sql)?.to_vec();
    normalize(&mut bytes, &btree)?;
    Ok(bytes)
}

pub(crate) fn normalize(image: &mut [u8], btree: &[u32]) -> io::Result<()> {
    let size = page_size(image).filter(|size| image.len().is_multiple_of(*size)).ok_or_else(|| invalid("not a whole SQLite image"))?;
    let count = image.len() / size;
    let usable = size - usize::from(image[20]);
    let page = |number: u32| -> io::Result<Range<usize>> {
        let number = number as usize;
        if number == 0 || number > count {
            return Err(invalid("a page reference lies outside the image"));
        }
        Ok((number - 1) * size..number * size)
    };
    image[18] = 1;
    image[19] = 1;
    let mut trunk = be32(&image[32..]);
    let mut hops = 0;
    while trunk != 0 {
        hops += 1;
        let range = page(trunk)?;
        let next = be32(&image[range.start..]);
        let leaves = be32(&image[range.start + 4..]) as usize;
        if hops > count || 8 + 4 * leaves > usable {
            return Err(invalid("the freelist is malformed"));
        }
        for index in 0..leaves {
            let leaf = page(be32(&image[range.start + 8 + 4 * index..]))?;
            image[leaf].fill(0);
        }
        image[range.start + 8 + 4 * leaves..range.end].fill(0);
        trunk = next;
    }
    for &number in btree {
        let range = page(number)?;
        clear_free_space(&mut image[range][..usable], if number == 1 { 100 } else { 0 })?;
    }
    Ok(())
}

fn clear_free_space(page: &mut [u8], header: usize) -> io::Result<()> {
    let corrupt = || invalid("a b-tree page header is malformed");
    let size = match page[header] {
        2 | 5 => 12,
        10 | 13 => 8,
        _ => return Err(corrupt()),
    };
    let cells = usize::from(be16(&page[header + 3..]));
    let content = match be16(&page[header + 5..]) {
        0 => 65536,
        start => usize::from(start),
    };
    let pointers = header + size + 2 * cells;
    if pointers > content || content > page.len() {
        return Err(corrupt());
    }
    page[pointers..content].fill(0);
    let mut block = usize::from(be16(&page[header + 1..]));
    let mut hops = 0;
    while block != 0 {
        hops += 1;
        if hops > page.len() / 4 || block + 4 > page.len() {
            return Err(corrupt());
        }
        let next = usize::from(be16(&page[block..]));
        let length = usize::from(be16(&page[block + 2..]));
        if length < 4 || block + length > page.len() {
            return Err(corrupt());
        }
        page[block + 4..block + length].fill(0);
        block = next;
    }
    Ok(())
}

fn hash_pages(file: &File) -> (usize, Vec<Hash>) {
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

fn hash_file(file: &File) -> io::Result<Hash> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    let mut offset = 0;
    loop {
        let read = file.read_at(&mut buffer, offset)?;
        if read == 0 {
            return Ok(hasher.finalize().into());
        }
        hasher.update(&buffer[..read]);
        offset += read as u64;
    }
}

pub(crate) fn manifest_bytes(size: usize, hashes: &[Hash]) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    bytes.extend((size as u32).to_be_bytes());
    bytes.extend((hashes.len() as u32).to_be_bytes());
    hashes.iter().for_each(|hash| bytes.extend(hash));
    bytes
}

fn read_manifest(input: &mut dyn Read) -> io::Result<(usize, Vec<Hash>)> {
    let mut magic = [0; MAGIC.len()];
    input.read_exact(&mut magic)?;
    if magic != MAGIC {
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
    Ok((size, hashes))
}

fn page_size(header: &[u8]) -> Option<usize> {
    if header.len() < 100 || !header.starts_with(SQLITE_HEADER) {
        return None;
    }
    match be16(&header[16..]) {
        1 => Some(65536),
        size if size >= 512 && size.is_power_of_two() => Some(usize::from(size)),
        _ => None,
    }
}

fn read_u32(input: &mut dyn Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}

fn be16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn hash_of(bytes: &[u8]) -> Hash {
    Sha256::digest(bytes).into()
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

fn sql(error: rusqlite::Error) -> io::Error {
    io::Error::other(error.to_string())
}
