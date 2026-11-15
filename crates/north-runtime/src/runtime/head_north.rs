//! `north runtime head-north`: runs the north that covers a checkout, the verb `bin/head-north` execs. It picks the live
//! runtime when it covers the code a check needs, else the north this host kept for the checkout's tree, else builds one
//! from the tree and keeps it under the cache confined runs share, and execs it holding a shared lease on its directory.

use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use north_runtime_records::failure::{self, Failure};
use rustix::fs::{FlockOperation, flock};
use rustix::io::{FdFlags, fcntl_setfd};

const KEEP: usize = 4;
const HOLD: Duration = Duration::from_secs(3600);
const ATTEMPTS: u32 = 5;
const REFUSED: u8 = 1;

struct Checkout {
    root: PathBuf,
    live: PathBuf,
    cache: PathBuf,
    code: Vec<String>,
    defer: bool,
    covered: String,
}

struct Picked {
    north: PathBuf,
    set_home: bool,
}

fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"))
}

fn no_verdict(message: impl Into<String>) -> Failure {
    failure::failed(failure::DEFERRED, message)
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim_end().to_owned())
}

fn is_tree(name: &str) -> bool {
    name.len() == 40 && name.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn writable(cache: &Path) -> bool {
    fs::create_dir_all(cache).is_ok() && tempfile::Builder::new().prefix(".probe.").tempfile_in(cache).is_ok()
}

fn confined() -> bool {
    fs::read_to_string("/proc/self/status").is_ok_and(|status| status.lines().any(|line| line.strip_prefix("NoNewPrivs:").is_some_and(|rest| rest.trim() == "1")))
}

fn lock(path: &Path, operation: FlockOperation) -> Option<File> {
    let file = File::options().create(true).append(true).open(path).or_else(|_| File::open(path)).ok()?;
    flock(&file, operation).ok()?;
    Some(file)
}

impl Checkout {
    fn clean_tree(&self) -> Option<String> {
        if let Ok(tree) = std::env::var("HEAD_NORTH_TREE") && is_tree(&tree) && self.cache.join(&tree).join("north").is_file() {
            return Some(tree);
        }
        git(&self.root, &["status", "--porcelain", "--untracked-files=no"]).filter(String::is_empty)?;
        git(&self.root, &["rev-parse", "HEAD^{tree}"])
    }

    fn cached(&self, tree: &str) -> PathBuf {
        self.cache.join(tree).join("north")
    }

    fn prune(&self) {
        let Ok(entries) = fs::read_dir(&self.cache) else { return };
        let mut trees: Vec<(SystemTime, String)> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let modified = entry.metadata().ok().filter(|meta| meta.is_dir())?.modified().ok()?;
                is_tree(&name).then_some((modified, name))
            })
            .collect();
        trees.sort_by_key(|tree| std::cmp::Reverse(tree.0));
        for (modified, old) in trees.into_iter().skip(KEEP) {
            if modified.elapsed().unwrap_or_default() < HOLD {
                continue;
            }
            let dir = self.cache.join(&old);
            let Some(_exclusive) = lock(&dir.join("lease"), FlockOperation::NonBlockingLockExclusive) else { continue };
            for path in [dir.join("north"), self.cache.join(format!("{old}.lock")), dir.join("lease")] {
                let _ = fs::remove_file(path);
            }
            let _ = fs::remove_dir(&dir);
        }
    }

    fn store(&self, built: &Path, tree: &str) -> std::io::Result<()> {
        let part = self.cache.join(format!("{tree}.part"));
        fs::copy(built, &part)?;
        fs::create_dir_all(self.cache.join(tree))?;
        fs::rename(&part, self.cached(tree))?;
        self.prune();
        Ok(())
    }

    fn touch(&self, tree: &str) {
        if let Ok(dir) = File::open(self.cache.join(tree)) {
            let _ = dir.set_modified(SystemTime::now());
        }
    }

    fn build(&self, tree: Option<&str>, reason: &str) -> Result<PathBuf, Failure> {
        let status = git(&self.root, &["status", "--porcelain"]).unwrap_or_default().lines().take(3).collect::<Vec<_>>().join(" ");
        eprintln!("head-north: building north from the tree: {} (git status: {status})", reason.lines().next().unwrap_or(""));
        let engine = std::env::var_os("HEAD_NORTH_ENGINE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/etc/buildbarn/store-generation"));
        if !engine.exists() && std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
            let artifact = tree.map(|tree| state_dir().join("north/runtime-artifacts").join(tree).join("north"));
            if let Some(artifact) = artifact.as_ref().filter(|artifact| artifact.is_file()) {
                return Ok(artifact.clone());
            }
            let tree = tree.ok_or_else(|| no_verdict("head-north: no build-ahead artifact for tree of this unclean checkout, so no verdict"))?;
            let mut ask = Command::new(&self.live);
            ask.args(["runtime", "build-ahead", "--fetch", "--tree", tree]);
            if std::env::var("HEAD_NORTH_TREE").as_deref() == Ok(tree) {
                ask.args(["--commit", &git(&self.root, &["rev-parse", "HEAD"]).unwrap_or_default()]).arg("--checkout").arg(&self.root);
            }
            let output = ask.output().ok().filter(|output| output.status.success()).ok_or_else(|| no_verdict(format!("head-north: no build-ahead artifact for tree {tree}, so no verdict")))?;
            return Ok(PathBuf::from(String::from_utf8_lossy(&output.stdout).trim_end()));
        }
        if !self.live.exists() {
            return std::env::current_exe().map_err(|error| no_verdict(format!("head-north: building north from the tree failed, so no older north runs in its place: {error}")));
        }
        let output = Command::new(&self.live).args(["runtime", "tree-build"]).stderr(std::process::Stdio::inherit()).output().map_err(Failure::from)?;
        match output.status.code() {
            Some(0) => Ok(PathBuf::from(String::from_utf8_lossy(&output.stdout).lines().last().unwrap_or_default())),
            Some(code @ (1 | 75)) => Err(failure::failed(code as u8, "")),
            other => Err(no_verdict(format!("head-north: the live runtime {} could not build north from the tree (exit {}), so no older north runs in its place", self.live.display(), other.map_or_else(|| "signal".to_owned(), |code| code.to_string())))),
        }
    }

    fn built_from_tree(&self, reason: &str) -> Result<Picked, Failure> {
        let Some(tree) = self.clean_tree() else {
            return Ok(Picked { north: self.build(None, reason)?, set_home: true });
        };
        fs::create_dir_all(&self.cache)?;
        let _building = lock(&self.cache.join(format!("{tree}.lock")), FlockOperation::LockExclusive);
        if self.cached(&tree).is_file() {
            eprintln!("head-north: running the north this host built from tree {}: {}", &tree[..12], reason.lines().next().unwrap_or(""));
            self.touch(&tree);
        } else {
            let built = self.build(Some(&tree), reason)?;
            self.store(&built, &tree)?;
        }
        Ok(Picked { north: self.cached(&tree), set_home: true })
    }

    fn covering(&self, tree: Option<String>) -> Result<Result<Picked, String>, Failure> {
        let mut covers = Command::new(&self.live);
        covers.args(["runtime", "covers", "--head", &self.covered]);
        if !self.code.is_empty() {
            covers.args(["--code", &self.code.join(",")]);
        }
        let output = covers.output()?;
        if !output.status.success() {
            return Ok(Err(String::from_utf8_lossy(&output.stderr).into_owned() + &String::from_utf8_lossy(&output.stdout)));
        }
        let north = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim_end());
        if let Some(tree) = tree.filter(|tree| git(&self.root, &["rev-parse", &format!("{}^{{tree}}", self.covered)]).as_deref() == Some(tree.as_str())) {
            fs::create_dir_all(&self.cache)?;
            let _building = lock(&self.cache.join(format!("{tree}.lock")), FlockOperation::LockExclusive);
            if !self.cached(&tree).is_file() {
                self.store(&north, &tree)?;
            }
        }
        Ok(Ok(Picked { north, set_home: false }))
    }

    fn pick(&self, shun: bool) -> Result<Picked, Failure> {
        let tree = self.clean_tree();
        let mut reason = format!("no live runtime at {}", self.live.display());
        if let Some(tree) = tree.as_ref().filter(|tree| self.code.is_empty() && self.cached(tree).is_file()) {
            eprintln!("head-north: running the north this host keeps for tree {}", &tree[..12]);
            self.touch(tree);
            return Ok(Picked { north: self.cached(tree), set_home: true });
        }
        if !shun && self.live.is_file() {
            match self.covering(tree)? {
                Ok(picked) => return Ok(picked),
                Err(why) => reason = why,
            }
        }
        if self.defer {
            return Err(failure::failed(REFUSED, format!("head-north: refused, no live runtime covers the checker code ({}); the lander runs this check without --no-build on a build host, which builds north from the tree", reason.lines().next().unwrap_or(""))));
        }
        if std::env::var_os("NORTH_OFFLOAD_SLOT").is_some() && !self.live.is_file() {
            return Ok(Picked { north: self.live.clone(), set_home: false });
        }
        self.built_from_tree(&reason)
    }

    fn lease(&self, north: &Path) -> Option<File> {
        let lease = lock(&north.parent()?.join("lease"), FlockOperation::LockShared)?;
        north.is_file().then_some(lease)
    }
}

fn leased_north(checkout: &Checkout) -> Result<(Picked, File), Failure> {
    let settle = Duration::from_secs(std::env::var("HEAD_NORTH_SETTLE").ok().and_then(|seconds| seconds.parse().ok()).unwrap_or(2));
    for attempt in 1..=ATTEMPTS {
        let picked = checkout.pick(false)?;
        if let Some(lease) = checkout.lease(&picked.north) {
            return Ok((picked, lease));
        }
        eprintln!("head-north: {} is not a whole runtime yet (another process may be building it); trying again", picked.north.parent().unwrap_or(Path::new("")).display());
        if attempt != ATTEMPTS {
            std::thread::sleep(settle);
        }
    }
    let stuck = checkout.pick(false)?.north;
    let dir = stuck.parent().unwrap_or(Path::new(""));
    if checkout.defer {
        return Err(failure::failed(REFUSED, format!("head-north: refused, the live runtime {} stayed unreadable", dir.display())));
    }
    if std::env::var_os("NORTH_OFFLOAD_SLOT").is_some() {
        return Err(no_verdict(format!("head-north: refused, no whole runtime at {} after waiting, and an offload slot never builds north from the tree it checks", dir.display())));
    }
    let picked = checkout.built_from_tree("the live runtime stayed unreadable")?;
    let lease = checkout.lease(&picked.north).ok_or_else(|| no_verdict("head-north: the north built from the tree cannot be leased"))?;
    Ok((picked, lease))
}

fn lane_exec_listed() -> bool {
    Command::new("north").args(["lane", "exec", "--help"]).output().is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("lane exec [--lane"))
}

pub(super) fn run(root: &Path, arguments: &[String]) -> Result<(), Failure> {
    let (mut code, mut defer, mut build_only) = (Vec::new(), false, false);
    let mut rest = arguments;
    if let [flag, path, tail @ ..] = rest && flag == "--code" {
        code.push(path.clone());
        rest = tail;
    }
    if let [flag, tail @ ..] = rest && flag == "--no-build" {
        defer = true;
        rest = tail;
    }
    if let [flag, tail @ ..] = rest && flag == "--build" {
        build_only = true;
        rest = tail;
    }
    let state = state_dir().join("north");
    let mut cache = state.join("head-north");
    if !writable(&cache) {
        if !confined() {
            return Err(no_verdict(format!("head-north: {} is not writable and this run is not confined, so it runs no north from the cache confined runs share", cache.display())));
        }
        cache = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")).join("north/head-north");
    }
    let covered = ["SAFE_PUSH_LAND_RUNTIME_HEAD", "SAFE_PUSH_LAND_HEAD"].iter().find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty())).unwrap_or_else(|| "HEAD".to_owned());
    let checkout = Checkout { root: root.to_owned(), live: state.join("current/north"), cache, code, defer, covered };
    if build_only {
        println!("{}", checkout.pick(false)?.north.display());
        return Ok(());
    }
    let (picked, lease) = leased_north(&checkout)?;
    let _ = fcntl_setfd(&lease, FdFlags::empty());
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"));
    let own = picked.north.starts_with(&checkout.cache) || picked.north.starts_with(root) || picked.north.starts_with(target);
    let mut command = if own && lane_exec_listed() {
        let mut command = Command::new("north");
        command.args(["lane", "exec", "--lane"]).arg(root).arg("--").arg(&picked.north);
        command
    } else {
        Command::new(&picked.north)
    };
    command.args(rest);
    if picked.set_home && std::env::var_os("NORTH_HOME").is_none() {
        command.env("NORTH_HOME", root);
    }
    let error = command.exec();
    Err(failure::failed(126, format!("head-north: cannot run {}: {error}", picked.north.display())))
}
