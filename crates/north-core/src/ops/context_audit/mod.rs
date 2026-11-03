mod report;
mod scan;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

pub use scan::{Audit, Kind, Role, audit_file};

const USAGE: &str = "usage: north context audit [--since DURATION] [--json] [--session ID_PREFIX] [--root PROJECTS_DIR]...
  DURATION is N followed by m, h or d (default 7d); roots default to ~/.local/state/north/profiles/claude-*/projects";
const THREADS: usize = 4;

pub type Startup = fn() -> Result<usize, String>;

struct Args {
    since: Duration,
    label: String,
    json: bool,
    session: Option<String>,
    roots: Vec<PathBuf>,
}

pub fn run(arguments: &[String], startup: Startup) -> u8 {
    let args = match arguments.split_first() {
        Some((command, rest)) if command == "audit" => match parse(rest) {
            Ok(args) => args,
            Err(message) => return usage(&message),
        },
        _ => return usage("expected the audit subcommand"),
    };
    let started = Instant::now();
    let startup_tokens = match startup() {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("north context audit: policy-gate measure failed: {error}");
            return 1;
        }
    };
    let roots = if args.roots.is_empty() { default_roots() } else { args.roots.clone() };
    let files = match session_files(&roots, args.since, args.session.as_deref()) {
        Ok(files) => files,
        Err(error) => {
            eprintln!("north context audit: {error}");
            return 1;
        }
    };
    let audits = audit_all(&files, startup_tokens * 4);
    let elapsed = started.elapsed();
    let text = if args.json { report::json(&audits, &args.label, startup_tokens, elapsed) } else { report::summary(&audits, &args.label, startup_tokens, elapsed) };
    println!("{text}");
    0
}

fn usage(message: &str) -> u8 {
    eprintln!("north context audit: {message}\n{USAGE}");
    2
}

fn parse(arguments: &[String]) -> Result<Args, String> {
    let mut args = Args { since: Duration::from_secs(7 * 86_400), label: "7d".to_owned(), json: false, session: None, roots: Vec::new() };
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--json" => args.json = true,
            "--since" => {
                let value = rest.next().ok_or("--since needs a duration")?;
                args.since = duration(value).ok_or("--since takes N followed by m, h or d")?;
                args.label = value.clone();
            }
            "--session" => args.session = Some(rest.next().ok_or("--session needs an id prefix")?.clone()),
            "--root" => args.roots.push(PathBuf::from(rest.next().ok_or("--root needs a directory")?)),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

pub fn duration(value: &str) -> Option<Duration> {
    let (count, unit) = value.split_at(value.len().checked_sub(1)?);
    let count: u64 = count.parse().ok()?;
    let seconds = match unit {
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => return None,
    };
    Some(Duration::from_secs(count * seconds))
}

fn default_roots() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let profiles = home.join(".local/state/north/profiles");
    let mut roots: Vec<PathBuf> = std::fs::read_dir(&profiles)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("claude-"))
                .map(|entry| entry.path().join("projects"))
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default();
    roots.sort();
    roots
}

pub fn session_files(roots: &[PathBuf], since: Duration, session: Option<&str>) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let cutoff = SystemTime::now().checked_sub(since).unwrap_or(SystemTime::UNIX_EPOCH);
    let mut out = Vec::new();
    for root in roots {
        let files = crate::ops::convo::discover::claude_session_files(root)?;
        for disk in &files {
            let logical = logical_path(disk);
            if disk != &logical && files.contains(&logical) {
                continue;
            }
            let stem = logical.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            if session.is_some_and(|prefix| !stem.starts_with(prefix)) {
                continue;
            }
            let recent = std::fs::metadata(disk).and_then(|meta| meta.modified()).is_ok_and(|modified| modified >= cutoff);
            if recent {
                out.push((disk.clone(), logical));
            }
        }
    }
    Ok(out)
}

fn logical_path(disk: &Path) -> PathBuf {
    let text = disk.to_string_lossy();
    PathBuf::from(text.strip_suffix(".zst").unwrap_or(&text))
}

pub fn audit_all(files: &[(PathBuf, PathBuf)], startup_chars: usize) -> Vec<Audit> {
    let next = AtomicUsize::new(0);
    let found = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..THREADS.min(files.len()) {
            scope.spawn(|| {
                while let Some((disk, logical)) = files.get(next.fetch_add(1, Ordering::Relaxed)) {
                    match audit_file(disk, logical, startup_chars) {
                        Ok(audit) => found.lock().expect("audit results").push(audit),
                        Err(error) => eprintln!("north context audit: skipped {}: {error}", disk.display()),
                    }
                }
            });
        }
    });
    let mut audits = found.into_inner().expect("audit results");
    audits.sort_by(|left, right| left.path.cmp(&right.path));
    audits
}
