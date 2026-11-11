//! `north hand-in` and `north finish`: the calls a session hands its lane in with. Each stages and commits exactly the named paths in the lane that holds them, refusing a path in another checkout or paths with no changes before anything is committed, then hands the lane to the road `safe-push --to main` takes, so every gate comes back unchanged. `hand-in` waits there for the lander's verdict (north#1158); `finish` first runs the session's acceptance command, commits and queues only when it passes, and returns at the ticket (north#1194); given `--files`, it first writes their whole contents into the session's own lane, the checkout holding its directory, refusing a path outside it before anything is written (north#1218).

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use super::enqueue::{default_branch, git};
use crate::land::scans::outgoing::denylist::Denylist;

const EARLY_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

pub const USAGE: &str = "usage: north hand-in --message TEXT [--] PATH...";
pub const FINISH_USAGE: &str = "usage: north finish --check COMMAND --message TEXT [--files JSON|-] [--] PATH...";

pub struct Lane {
    pub root: PathBuf,
    pub destination: String,
}

/// Where a handed-in lane goes once its paths are committed. It may refuse a lane before anything is staged, as a worker sandbox refuses a lane whose git lock files it could create but not remove (north#1172).
pub trait Road {
    fn writable(&self, _root: &Path) -> Result<(), String> {
        Ok(())
    }

    fn take(self, lane: &Lane) -> u8;
}

impl<F: FnOnce(&Lane) -> u8> Road for F {
    fn take(self, lane: &Lane) -> u8 {
        self(lane)
    }
}

#[derive(serde::Deserialize)]
struct File {
    path: PathBuf,
    content: String,
}

struct Request<'a> {
    check: Option<&'a str>,
    message: &'a str,
    paths: Vec<PathBuf>,
    files: Vec<File>,
}

struct Named {
    lane: Lane,
    paths: Vec<String>,
    targets: Vec<PathBuf>,
    committed: bool,
}

pub fn run(arguments: &[String], road: impl Road) -> u8 {
    let request = match arguments {
        [flag, message, rest @ ..] if flag == "--message" => Request { check: None, message, paths: paths(rest), files: Vec::new() },
        _ => return usage(arguments, USAGE),
    };
    hand(&request, "hand-in", road)
}

pub fn finish(arguments: &[String], road: impl Road) -> u8 {
    let (check, message, rest) = match arguments {
        [check_flag, check, message_flag, message, rest @ ..] if check_flag == "--check" && message_flag == "--message" => (check, message, rest),
        _ => return usage(arguments, FINISH_USAGE),
    };
    let (files, rest) = match rest {
        [flag, source, rest @ ..] if flag == "--files" => match files(source) {
            Ok(files) => (files, rest),
            Err(refusal) => {
                eprintln!("finish: ✗ {refusal}; NOT written, NOT committed.");
                return 1;
            }
        },
        _ => (Vec::new(), rest),
    };
    hand(&Request { check: Some(check), message, paths: paths(rest), files }, "finish", road)
}

fn files(source: &str) -> Result<Vec<File>, String> {
    let text = if source == "-" { io::read_to_string(io::stdin()) } else { fs::read_to_string(source) };
    let text = text.map_err(|error| format!("the files in {source}: {error}"))?;
    serde_json::from_str(&text).map_err(|error| format!("the files in {source} are not a JSON list of {{\"path\", \"content\"}}: {error}"))
}

fn usage(arguments: &[String], usage: &str) -> u8 {
    if matches!(arguments, [flag] if flag == "-h" || flag == "--help") {
        println!("{usage}");
        return 0;
    }
    eprintln!("{usage}");
    64
}

fn paths(rest: &[String]) -> Vec<PathBuf> {
    rest.strip_prefix(&["--".to_owned()][..]).unwrap_or(rest).iter().map(PathBuf::from).collect()
}

fn hand(request: &Request, verb: &str, road: impl Road) -> u8 {
    let named = std::env::current_dir().map_err(|error| format!("current directory: {error}")).and_then(|directory| name(&directory, request, &road)).and_then(|named| apply(named, &request.files));
    let named = match named {
        Ok(named) => named,
        Err(refusal) => {
            eprintln!("{verb}: ✗ {refusal}; NOT committed.");
            return 1;
        }
    };
    if request.check.is_some() {
        ask_early(&named, request.message);
    }
    if let Some(check) = request.check
        && let Err(failure) = passes(&named.lane.root, check)
    {
        let kept = if request.files.is_empty() { "" } else { "the files stay as written; " };
        eprintln!("{verb}: ✗ the check `{check}` {failure}; {kept}NOT committed, NOT queued.");
        return 1;
    }
    match commit(&named, request.message) {
        Ok(()) => road.take(&named.lane),
        Err(refusal) => {
            eprintln!("{verb}: ✗ {refusal}; NOT committed.");
            1
        }
    }
}

fn passes(root: &Path, check: &str) -> Result<(), String> {
    if check.trim().is_empty() {
        return Err("is empty".to_owned());
    }
    let status = Command::new("sh").arg("-c").arg(check).current_dir(root).stdin(Stdio::null()).status().map_err(|error| format!("did not start: {error}"))?;
    match status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(format!("exited {code}")),
        None => Err("was killed by a signal".to_owned()),
    }
}

fn name(directory: &Path, request: &Request, road: &impl Road) -> Result<Named, String> {
    if request.message.trim().is_empty() {
        return Err("the message is empty".to_owned());
    }
    let targets: Vec<PathBuf> = request.files.iter().map(|file| normal(&directory.join(&file.path))).collect();
    let paths: Vec<PathBuf> = request.paths.iter().map(|path| normal(&directory.join(path))).chain(targets.iter().cloned()).collect();
    let Some(first) = paths.first() else { return Err("name the paths to hand in".to_owned()) };
    let home = if targets.is_empty() { first.as_path() } else { directory };
    let root = checkout(home).ok_or_else(|| format!("{} is in no git checkout", home.display()))?;
    if let Some(outside) = paths.iter().find(|path| checkout(path).as_ref() != Some(&root)) {
        return Err(format!("{} is outside the lane {}", outside.display(), root.display()));
    }
    road.writable(&root)?;
    let destination = default_branch(&root);
    match git(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Ok(branch) if branch != destination => {}
        _ => return Err(format!("{} is not on a lane's own branch", root.display())),
    }
    let named: Vec<String> = paths.iter().map(|path| path.to_str().map(str::to_owned).ok_or_else(|| "a path is not valid UTF-8".to_owned())).collect::<Result<_, _>>()?;
    let relative: Vec<String> = paths.iter().map(|path| relative_to(&root, path).ok_or_else(|| format!("{} is outside the lane {}", path.display(), root.display()))).collect::<Result<_, _>>()?;
    let committed = lane_changes(&root, &destination)?;
    let unnamed: Vec<&str> = committed.iter().map(String::as_str).filter(|change| !relative.iter().any(|path| path == change)).collect();
    if !unnamed.is_empty() {
        return Err(format!("the lane's commits also change {}, which the hand-in did not name", unnamed.join(" ")));
    }
    Ok(Named { lane: Lane { root, destination }, paths: named, targets, committed: !committed.is_empty() })
}

fn apply(named: Named, files: &[File]) -> Result<Named, String> {
    for (target, file) in named.targets.iter().zip(files) {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        fs::write(target, &file.content).map_err(|error| format!("{}: {error}", target.display()))?;
    }
    let paths: Vec<&str> = named.paths.iter().map(String::as_str).collect();
    if !named.committed && git(&named.lane.root, &[&["status", "--porcelain", "--untracked-files=all", "--"][..], &paths].concat())?.is_empty() {
        return Err(format!("no changes in {}", paths.join(" ")));
    }
    Ok(named)
}

fn commit(named: &Named, message: &str) -> Result<(), String> {
    let (root, paths) = (&named.lane.root, named.paths.iter().map(String::as_str).collect::<Vec<_>>());
    git(root, &[&["add", "--all", "--"][..], &paths].concat())?;
    if git(root, &[&["diff", "--cached", "--name-only", "HEAD", "--"][..], &paths].concat())?.is_empty() {
        return if named.committed { Ok(()) } else { Err(format!("no changes in {}", paths.join(" "))) };
    }
    git(root, &[&["commit", "--quiet", "--only", "--message", message, "--"][..], &paths].concat()).map(drop)
}

fn lane_changes(root: &Path, destination: &str) -> Result<Vec<String>, String> {
    let range = format!("refs/remotes/origin/{destination}...HEAD");
    Ok(git(root, &["diff", "--name-only", &range, "--"])?.lines().map(str::to_owned).collect())
}

fn relative_to(root: &Path, path: &Path) -> Option<String> {
    let existing = path.ancestors().find(|ancestor| ancestor.exists())?;
    let rest = path.strip_prefix(existing).ok()?;
    let real = existing.canonicalize().ok()?;
    let real = if rest.as_os_str().is_empty() { real } else { real.join(rest) };
    real.strip_prefix(root).ok()?.to_str().map(str::to_owned)
}

fn checkout(path: &Path) -> Option<PathBuf> {
    let existing = path.ancestors().find(|ancestor| ancestor.is_dir())?;
    git(existing, &["rev-parse", "--show-toplevel"]).ok().map(PathBuf::from)
}

fn normal(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    normal
}

fn ask_early(named: &Named, message: &str) {
    let root = named.lane.root.clone();
    let destination = named.lane.destination.clone();
    let Some(head) = would_be(&root, message, &named.paths) else { return };
    std::thread::spawn(move || {
        let Ok(origin) = git(&root, &["remote", "get-url", "--push", "origin"]) else { return };
        let base = format!("refs/remotes/origin/{destination}");
        let _ = Denylist::from_env().verdict_within(&root, &origin, (&base, &head, &destination), Some(EARLY_WAIT));
    });
}

fn would_be(root: &Path, message: &str, paths: &[String]) -> Option<String> {
    let index = PathBuf::from(git(root, &["rev-parse", "--path-format=absolute", "--git-path", "index"]).ok()?);
    let scratch = index.with_file_name(format!("index.finish-{}", std::process::id()));
    let staged = |arguments: &[&str], input: Option<&str>| apart(root, &scratch, arguments, input);
    let made = (|| {
        staged(&["read-tree", "HEAD"], None)?;
        staged(&[&["add", "--all", "--"][..], &paths.iter().map(String::as_str).collect::<Vec<_>>()].concat(), None)?;
        let tree = staged(&["write-tree"], None)?;
        let cleaned = staged(&["stripspace"], Some(message))?;
        staged(&["commit-tree", tree.trim(), "-p", "HEAD", "-F", "-"], Some(&cleaned))
    })();
    let _ = std::fs::remove_file(&scratch);
    made.map(|head| head.trim().to_owned())
}

fn apart(root: &Path, index: &Path, arguments: &[&str], input: Option<&str>) -> Option<String> {
    let mut child = Command::new("git").arg("-C").arg(root).args(arguments).env("GIT_INDEX_FILE", index).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    std::io::Write::write_all(&mut stdin, input.unwrap_or_default().as_bytes()).ok()?;
    drop(stdin);
    let output = child.wait_with_output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
