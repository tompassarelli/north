//! Release runtimes built ahead of the landing they belong to: the lander asks for the release north of a queued
//! lane's candidate tree, a build host builds it, and the promotion installs it by the tree hash the landed commit
//! carries instead of building the same tree again after the push. An artifact is kept under
//! `runtime-artifacts/<tree>`: the build host that holds the programs, the commit and the host they were built from and
//! on, and when; the programs, about 97 MB, stay on that build host until the promotion of the landing pulls them, so
//! a landing copies none of them here and the promotion copies them once, straight into its runtime (north#1362).
//! Each request is a file under `runtime-artifacts/requests` named by its tree. Each build host has one slot, a lock
//! file here, and whoever holds a slot drains the requests in order, every lander candidate oldest first and then a
//! queued lane's hand-in, taking each off the queue so no other slot builds it, while a newer request withdraws every older
//! one still waiting, so only the newest waiting tree is built ahead: requests build side by side, one per
//! build host, and a candidate waits only while every build host is building; none builds on this host. The newest nine
//! artifacts are kept, since builds side by side end three at a time.
use super::super::build_host;
use super::super::failure::{Failure, failed};
use super::super::layout::{self, BUILT_FROM, BUILT_ON, HELD_ON, Layout, PROGRAMS};
use super::super::pin::{self, Event};
use super::super::pull;
use super::super::remote;
use north_offload::land_offload::{self, BuildHost};
use crate::platform::process;
use rustix::fs::{FlockOperation, flock};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const BUILT_AT: &str = "built-at";
const BUILDING: &str = "building.lock";
const KIND: &str = "kind";
const SUPERSEDED: &str = "superseded";
const PART: &str = ".part";
type Stop<'a> = &'a dyn Fn() -> bool;
const REQUESTS: &str = "requests";
const STALE_REQUEST: Duration = Duration::from_secs(24 * 60 * 60);
const SLOT_POLL: Duration = Duration::from_secs(2);
const KEPT: usize = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Candidate,
    HandIn,
}

impl Kind {
    fn parse(word: &str) -> Option<Self> {
        match word {
            "candidate" => Some(Self::Candidate),
            "hand-in" => Some(Self::HandIn),
            _ => None,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::HandIn => "hand-in",
        }
    }
}

/// One request for the release north of `tree`, built from `commit`; `lane` names the queued lane a hand-in came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ask {
    pub(super) tree: String,
    pub(super) commit: String,
    pub(super) kind: Kind,
    pub(super) lane: String,
}
const SEND: Duration = Duration::from_secs(120);
const AWAIT: Duration = Duration::from_secs(600);

/// The release north kept for the tree `rev` in `checkout` carries, when it is ready: the artifact is the whole
/// install, so a promotion can copy it and skip the build that would otherwise delay the landing's live runtime.
pub(super) fn ready_for(layout: &Layout, checkout: &Path, rev: &str) -> Option<String> {
    let tree = tree_of(checkout, rev).ok()?;
    ready(layout, &tree).map(|_| tree)
}

/// The tree hash of `rev`, the identity a candidate carries from the lander's queue to the landed commit.
pub(super) fn tree_of(checkout: &Path, rev: &str) -> Result<String, Failure> {
    let output = process::run("git", Some(checkout), &["rev-parse", &format!("{rev}^{{tree}}")])?;
    if !output.status.success() {
        return Err(failed(1, format!("git rev-parse {rev}^{{tree}}: {}", String::from_utf8_lossy(&output.stderr).trim())));
    }
    let tree = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    layout::is_sha(&tree).then_some(tree).ok_or_else(|| failed(1, format!("git rev-parse {rev}^{{tree}} printed no tree hash")))
}

/// The artifact for `tree`, when it names the build host holding its build.
pub(super) fn ready(layout: &Layout, tree: &str) -> Option<PathBuf> {
    let artifact = layout.artifacts().join(tree);
    layout::held(&artifact).then_some(artifact)
}

/// Whether the artifact for `tree` is ready, waiting while a build host builds it now: a coordinator orders main's head
/// minutes after it lands, often while its build ahead still runs, and a second build would cost the order those minutes
/// again. A request still queued is withdrawn rather than waited for: the promotion builds the tree itself, and the queued
/// request would build it a second time once the promotion ends, holding the build lock meanwhile.
pub(super) fn awaited(layout: &Layout, tree: &str) -> bool {
    let started = std::time::Instant::now();
    let mut announced = false;
    loop {
        if ready(layout, tree).is_some() {
            return true;
        }
        if !building(layout, tree) {
            withdraw(layout, tree);
            return false;
        }
        if started.elapsed() >= AWAIT {
            return false;
        }
        if !announced {
            println!("north runtime: tree {} is being built ahead on a build host; waiting up to {}s for it", layout::short(tree), AWAIT.as_secs());
            announced = true;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn withdraw(layout: &Layout, tree: &str) {
    let _ = fs::remove_file(layout.artifacts().join(REQUESTS).join(tree));
}

fn building(layout: &Layout, tree: &str) -> bool {
    File::open(staging(&layout.artifacts(), tree).join(BUILDING)).is_ok_and(|file| flock(&file, FlockOperation::NonBlockingLockShared).is_err())
}

fn staging(artifacts: &Path, tree: &str) -> PathBuf {
    artifacts.join(format!("{tree}{PART}"))
}

/// Installs the artifact for `tree` as `runtime`, named for the landed commit `target`, when it is ready and the commit it was built from carries `tree`: `fetch` copies the programs and their stamp from the build host the artifact names into `runtime`, the one copy of the release north this host takes; whether it did.
pub(super) fn adopt(layout: &Layout, runtime: &Path, tree: &str, target: &str, tree_of: impl Fn(&str) -> Option<String>, fetch: impl FnOnce(&str, &str, &Path) -> Result<(), String>) -> bool {
    let Some(artifact) = ready(layout, tree) else { return false };
    let stamp = fs::read_to_string(artifact.join(BUILT_FROM)).unwrap_or_default();
    let Some(commit) = stamp.split_whitespace().next().filter(|commit| tree_of(commit).as_deref() == Some(tree)) else {
        println!("north runtime: the artifact kept for tree {} was built from a commit that does not carry it, so it is not installed", layout::short(tree));
        return false;
    };
    let installed = match pulled(&artifact, &stamp, commit, runtime, fetch) {
        Ok(()) => install(&artifact, runtime, target),
        Err(why) => {
            println!("north runtime: the release north built ahead for tree {} could not be pulled ({why}), so the promotion has a build host build it", layout::short(tree));
            for program in PROGRAMS.iter().chain([&BUILT_FROM]) {
                let _ = fs::remove_file(runtime.join(program));
            }
            let _ = fs::remove_dir_all(&artifact);
            false
        }
    };
    if installed {
        let _ = fs::remove_dir_all(&artifact);
        pin::note(layout, &Event::ArtifactReady { sha: target.to_owned(), tree: tree.to_owned(), seconds: 0, at: pin::now() });
        println!("north runtime: artifact-ready 0 s: the release north for tree {} was built ahead while its lane queued", layout::short(tree));
    }
    installed
}

fn pulled(artifact: &Path, stamp: &str, commit: &str, runtime: &Path, fetch: impl FnOnce(&str, &str, &Path) -> Result<(), String>) -> Result<(), String> {
    let host = fs::read_to_string(artifact.join(HELD_ON)).map_err(|error| format!("{HELD_ON}: {error}"))?;
    fetch(host.trim(), commit, runtime)?;
    let recorded = stamp.split_whitespace().nth(1).unwrap_or_default();
    let pulled = fs::read_to_string(runtime.join(BUILT_FROM)).is_ok_and(|pulled| pulled == stamp) && layout::binary_hash(&runtime.join("north")).is_ok_and(|hash| hash == recorded);
    pulled.then_some(()).ok_or_else(|| format!("{} now holds another build of {}", host.trim(), layout::short(commit)))
}

/// `north runtime build-ahead --fetch --tree TREE`: prints the path of the north built ahead for `tree` that this host can run, first pulling it from the build host its artifact names when that host alone holds it, so a landing step that needs the merged tree's north runs on a lander with no build grid (north#1433).
fn fetched_north(layout: &Layout, tree: &str) -> Result<(), Failure> {
    let artifact = layout.artifacts().join(tree);
    let north = artifact.join("north");
    if !north.is_file() {
        let stamp = fs::read_to_string(artifact.join(BUILT_FROM)).map_err(|error| failed(75, format!("no build-ahead artifact for tree {}: {error}", layout::short(tree))))?;
        let commit = stamp.split_whitespace().next().unwrap_or_default().to_owned();
        let staged = artifact.join(format!("pulled-{}.part", std::process::id()));
        let fetched = pulled(&artifact, &stamp, &commit, &staged, |host, commit, into| pull::fetch(host, commit, into, &[BUILT_FROM])).and_then(|()| fs::rename(staged.join("north"), &north).map_err(|error| error.to_string()));
        let _ = fs::remove_dir_all(&staged);
        fetched.map_err(|why| failed(75, format!("the north built ahead for tree {} could not be pulled: {why}", layout::short(tree))))?;
    }
    println!("{}", north.display());
    Ok(())
}

fn install(artifact: &Path, runtime: &Path, target: &str) -> bool {
    layout::stamp(runtime, target).is_ok() && fs::copy(artifact.join(BUILT_ON), runtime.join(BUILT_ON)).is_ok()
}

/// `north runtime build-ahead --tree TREE --commit COMMIT [--checkout DIR]`: the lander's request for the release
/// north a lane's candidate tree carries, kept where the promotion of that tree's landing finds it.
pub(in crate::runtime) fn run(layout: &Layout, flags: &[String]) -> Result<(), Failure> {
    if let [fetch, tree_flag, tree] = flags
        && fetch == "--fetch"
        && tree_flag == "--tree"
        && layout::is_sha(tree)
    {
        return fetched_north(layout, tree);
    }
    let mut tree = None;
    let mut commit = None;
    let mut kind = Kind::Candidate;
    let mut lane = String::new();
    let mut checkout = std::env::current_dir().map_err(|error| failed(64, format!("current directory: {error}")))?;
    let mut words = flags.iter();
    while let Some(flag) = words.next() {
        let value = words.next().cloned().ok_or_else(|| failed(64, format!("{flag} needs a value")))?;
        match flag.as_str() {
            "--tree" => tree = Some(value),
            "--commit" => commit = Some(value),
            "--checkout" => checkout = PathBuf::from(value),
            "--kind" => kind = Kind::parse(&value).ok_or_else(|| failed(64, format!("--kind {value}: candidate or hand-in")))?,
            "--lane" => lane = value,
            other => return Err(failed(64, format!("unknown build-ahead flag {other}"))),
        }
    }
    let (Some(tree), Some(commit)) = (tree, commit) else {
        return Err(failed(64, "build-ahead needs --tree TREE --commit COMMIT"));
    };
    build(layout, &checkout, &Ask { tree, commit, kind, lane })
}

/// Has a build host build the release north `ask` names, with every request waiting before it, and keeps each as the
/// promotion's artifact; a tree another request already fills, or one ready already, costs nothing.
pub(super) fn build(layout: &Layout, checkout: &Path, ask: &Ask) -> Result<(), Failure> {
    request(layout, ask, land_offload::build_host_rooms, |host, commit, into, stop| {
        send(checkout, host, commit)?;
        build_host::build_held(host, commit, into, stop)
    })
}

/// Files `ask`, then waits for a free build host slot and drains every request in order on that host, or ends once
/// another slot took `ask` off the queue or a promotion withdrew it, so requests build side by side, one per build host,
/// and each still waiting has a process waiting for a slot. This host settles the fleet while people work on it, so it
/// never builds the release itself: with no build host answering, or every one failing, nothing is kept and the
/// promotion of the landing has a build host build it.
fn request(layout: &Layout, ask: &Ask, hosts: impl Fn() -> Vec<BuildHost>, mut build_on: impl FnMut(&BuildHost, &str, &Path, Stop) -> Result<(), Failure>) -> Result<(), Failure> {
    if !layout::is_sha(&ask.tree) || !layout::is_sha(&ask.commit) {
        return Err(failed(64, "build-ahead needs a tree hash and a commit hash"));
    }
    let artifacts = layout.artifacts();
    file(&artifacts, ask)?;
    let answered = match build_host::answered(hosts()) {
        Ok(answered) => answered,
        Err(why) => {
            withdraw(layout, &ask.tree);
            println!("north runtime build-ahead: {why}; this host builds no release ahead, so the promotion of tree {} has a build host build it", layout::short(&ask.tree));
            return Ok(());
        }
    };
    let mut announced = false;
    loop {
        if let Some((held, _slot)) = answered.iter().find_map(|host| slot(&artifacts, host).map(|slot| (host, slot))) {
            while let Some(next) = claim(&artifacts) {
                built_ahead(layout, &artifacts, &next, held, &answered, &mut build_on)?;
            }
            return Ok(());
        }
        if !artifacts.join(REQUESTS).join(&ask.tree).is_file() {
            return Ok(());
        }
        if !announced {
            println!("north runtime build-ahead: every build host is building ahead; tree {} waits for the first to come free", layout::short(&ask.tree));
            announced = true;
        }
        std::thread::sleep(SLOT_POLL);
    }
}

/// The slot of build host `host`, when no other request is building on it: one build ahead per build host, since a
/// build host runs one `north runtime build` at a time and a second would wait there instead of here.
fn slot(artifacts: &Path, host: &BuildHost) -> Option<File> {
    let name: String = host.name.chars().map(|char| if char.is_ascii_alphanumeric() || char == '-' { char } else { '_' }).collect();
    let file = File::create(artifacts.join(format!("build-{name}.lock"))).ok()?;
    flock(&file, FlockOperation::NonBlockingLockExclusive).ok().map(|()| file)
}

/// Writes the request file named by the validated tree hash alone, and withdraws every other request still waiting: only
/// the newest waiting tree is built ahead, since an older one is a head the lander has since superseded or reverted, and its
/// build would run to the end for a landing that never comes. A hand-in never withdraws a candidate, and a hand-in for a
/// tree a candidate asked for leaves the candidate first in line.
fn file(artifacts: &Path, ask: &Ask) -> Result<(), Failure> {
    let requests = artifacts.join(REQUESTS);
    fs::create_dir_all(&requests)?;
    let filed = filed(artifacts);
    if ask.kind == Kind::HandIn && filed.iter().any(|(tree, filed)| *tree == ask.tree && filed.kind == Kind::Candidate) {
        return Ok(());
    }
    for (tree, filed) in &filed {
        if *tree != ask.tree && !(ask.kind == Kind::HandIn && filed.kind == Kind::Candidate) {
            let _ = fs::remove_file(requests.join(tree));
        }
    }
    supersede_running(artifacts, ask);
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    let staged = requests.join(format!(".{}.new", ask.tree));
    fs::write(&staged, format!("{}\n{}\n{at}\n{}\n", ask.kind.word(), ask.commit, ask.lane))?;
    fs::rename(&staged, requests.join(&ask.tree))?;
    Ok(())
}

/// Marks every build running for another tree as superseded, by the rule that withdraws a waiting request: its build
/// host build ends at once, since the landing it was for never comes.
fn supersede_running(artifacts: &Path, ask: &Ask) {
    for entry in fs::read_dir(artifacts).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(tree) = name.strip_suffix(PART).filter(|tree| *tree != ask.tree) else { continue };
        let running = fs::read_to_string(entry.path().join(KIND)).ok().and_then(|word| Kind::parse(word.trim()));
        if layout::is_sha(tree) && running.is_some_and(|kind| !(ask.kind == Kind::HandIn && kind == Kind::Candidate)) {
            let _ = fs::write(entry.path().join(SUPERSEDED), "");
        }
    }
}

/// Every request filed, with its tree, its ask and when it was filed; a file that is no request or is a day old is dropped.
fn filed(artifacts: &Path) -> Vec<(String, Filed)> {
    let requests = artifacts.join(REQUESTS);
    let mut filed = Vec::new();
    for entry in fs::read_dir(&requests).into_iter().flatten().flatten() {
        let tree = entry.file_name().to_string_lossy().into_owned();
        if !layout::is_sha(&tree) {
            continue;
        }
        let stale = entry.metadata().ok().and_then(|metadata| metadata.modified().ok()).and_then(|modified| modified.elapsed().ok()).is_some_and(|age| age > STALE_REQUEST);
        match fs::read_to_string(entry.path()).ok().and_then(|text| Filed::parse(&text)).filter(|_| !stale) {
            Some(request) => filed.push((tree, request)),
            None => {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    filed
}

struct Filed {
    kind: Kind,
    commit: String,
    at: u128,
    lane: String,
}

impl Filed {
    fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let kind = Kind::parse(lines.next()?)?;
        let commit = lines.next().filter(|commit| layout::is_sha(commit))?.to_owned();
        let at = lines.next()?.parse().ok()?;
        Some(Self { kind, commit, at, lane: lines.next().unwrap_or_default().to_owned() })
    }
}

/// The request built next, taken off the queue so no other slot builds it: the oldest candidate, else the oldest
/// hand-in, so a candidate waits only while every build host is building.
fn claim(artifacts: &Path) -> Option<Ask> {
    loop {
        let (tree, filed) = filed(artifacts).into_iter().min_by_key(|(_, filed)| (filed.kind == Kind::HandIn, filed.at))?;
        match fs::remove_file(artifacts.join(REQUESTS).join(&tree)) {
            Ok(()) => return Some(Ask { tree, commit: filed.commit, kind: filed.kind, lane: filed.lane }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
    }
}

fn built_ahead(layout: &Layout, artifacts: &Path, ask: &Ask, held: &BuildHost, answered: &[BuildHost], build_on: &mut impl FnMut(&BuildHost, &str, &Path, Stop) -> Result<(), Failure>) -> Result<(), Failure> {
    let (tree, commit) = (ask.tree.as_str(), ask.commit.as_str());
    if ready(layout, tree).is_some() {
        println!("north runtime build-ahead: tree {} is ready already", layout::short(tree));
        return Ok(());
    }
    let staging = staging(artifacts, tree);
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let building = File::create(staging.join(BUILDING))?;
    flock(&building, FlockOperation::NonBlockingLockExclusive).map_err(std::io::Error::from)?;
    fs::write(staging.join(KIND), ask.kind.word())?;
    let taken = staging.join(commit);
    let stop = || staging.join(SUPERSEDED).is_file();
    let kept = built_on_a_slot(artifacts, commit, &taken, held, answered, build_on, &stop).and_then(|built| keep(&taken, &artifacts.join(tree)).map(|()| built).map_err(|error| format!("could not keep the build under {}: {error}", artifacts.display())));
    let superseded = stop();
    let _ = fs::remove_dir_all(&staging);
    match kept {
        Err(_) if superseded => println!("north runtime build-ahead: the build of tree {} ended at once: a newer request superseded it", layout::short(tree)),
        Ok(built) => {
            prune(artifacts);
            println!("north runtime build-ahead: tree {} built on {} in {}s, ready for the promotion of {} ({})", layout::short(tree), built.host, built.seconds, layout::short(commit), ask.kind.word());
        }
        Err(why) => println!("north runtime build-ahead: {why}; this host builds no release ahead, so the promotion of tree {} has a build host build it", layout::short(tree)),
    }
    Ok(())
}

/// The ref a candidate commit is sent to a build host under: it has not landed, so the host's `north runtime build`
/// takes this ref in place of origin/main.
fn candidate_ref(commit: &str) -> String {
    format!("refs/build-ahead/{commit}")
}

fn send(checkout: &Path, host: &BuildHost, commit: &str) -> Result<(), Failure> {
    let mut push = Command::new("git");
    push.arg("-C").arg(checkout).args(["push", "-q", "--no-verify"]).arg(format!("{}:{}", host.ssh, super::CHECKOUT)).arg(format!("{commit}:{}", candidate_ref(commit))).env("GIT_SSH_COMMAND", format!("{} -o BatchMode=yes -o ConnectTimeout=20", remote::OWN_RSH));
    remote::run(&host.name, "sending the candidate commit", &mut push, SEND)
}

/// On a build host: whether `commit` was sent here as a lander's candidate.
pub(super) fn sent(checkout: &Path, commit: &str) -> bool {
    process::run("git", Some(checkout), &["rev-parse", "--verify", "--quiet", &format!("{}^{{commit}}", candidate_ref(commit))]).is_ok_and(|output| output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == commit)
}

/// On a build host: drops the ref a candidate was sent under once its build ended, so the refs never pile up.
pub(super) fn forget(checkout: &Path, commit: &str) {
    let _ = process::run("git", Some(checkout), &["update-ref", "-d", &candidate_ref(commit)]);
}

/// Completes the artifact a build host's programs were taken into, then gives it the name a promotion finds by tree
/// hash. A promotion reading it never sees a half-written one.
fn keep(taken: &Path, artifact: &Path) -> std::io::Result<()> {
    fs::write(taken.join(BUILT_AT), format!("{}\n", pin::now()))?;
    let _ = fs::remove_dir_all(artifact);
    fs::rename(taken, artifact)
}

fn prune(artifacts: &Path) {
    let mut kept: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(artifacts)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()) && layout::is_sha(&entry.file_name().to_string_lossy()))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    kept.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, path) in kept.into_iter().skip(KEPT) {
        println!("north runtime build-ahead: dropped the artifact for tree {}", path.file_name().unwrap_or_default().to_string_lossy());
        let _ = fs::remove_dir_all(&path);
    }
}

fn slot_beside(artifacts: &Path, host: &BuildHost, held: &BuildHost) -> Option<Option<File>> {
    match host.name == held.name {
        true => Some(None),
        false => slot(artifacts, host).map(Some),
    }
}

/// Builds `commit` on the build host whose slot is held, then, while one fails, on each other build host that answered
/// and whose slot is free, so one build host's failure leaves the tree to another, never two builds on one host.
fn built_on_a_slot(artifacts: &Path, commit: &str, taken: &Path, held: &BuildHost, answered: &[BuildHost], build_on: &mut impl FnMut(&BuildHost, &str, &Path, Stop) -> Result<(), Failure>, stop: Stop) -> Result<build_host::Built, String> {
    let mut refused = Vec::new();
    for host in std::iter::once(held).chain(answered.iter().filter(|host| host.name != held.name)) {
        if stop() {
            break;
        }
        let Some(_slot) = slot_beside(artifacts, host, held) else { continue };
        println!("north runtime: building {} on {}, a build host with no build ahead running ({:.0} free cores)", layout::short(commit), host.name, host.free_cpus.unwrap_or_default());
        match held_on_host(host, commit, taken, build_on, stop) {
            Ok(built) => return Ok(built),
            Err(why) => refused.push(why),
        }
    }
    Err(refused.join("; "))
}

/// Has `host` build `commit` and records in `taken` which host holds the build and the stamp it read there, with no program copied here.
fn held_on_host(host: &BuildHost, commit: &str, taken: &Path, build_on: &mut impl FnMut(&BuildHost, &str, &Path, Stop) -> Result<(), Failure>, stop: Stop) -> Result<build_host::Built, String> {
    let started = std::time::Instant::now();
    let refused = |why: String| format!("{} did not build {}: {why}", host.name, layout::short(commit));
    build_on(host, commit, taken, stop).map_err(|failure| refused(failure.message))?;
    let stamp = fs::read_to_string(taken.join(BUILT_FROM)).map_err(|error| refused(format!("{BUILT_FROM}: {error}")))?;
    let binary = match stamp.split_whitespace().collect::<Vec<_>>()[..] {
        [built, binary] if built == commit => binary.to_owned(),
        _ => return Err(refused(format!("its {BUILT_FROM} names another commit"))),
    };
    fs::write(taken.join(HELD_ON), format!("{}\n", host.ssh)).and_then(|()| fs::write(taken.join(BUILT_ON), format!("{} {binary}\n", host.name))).map_err(|error| refused(error.to_string()))?;
    Ok(build_host::Built { host: host.name.clone(), seconds: started.elapsed().as_secs() as i64, binary })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_there(hosts: &Path, host: &BuildHost, commit: &str, into: &Path, _: Stop) -> Result<(), Failure> {
        let kept = hosts.join(&host.ssh).join(commit);
        fs::create_dir_all(&kept)?;
        for program in PROGRAMS {
            fs::write(kept.join(program), format!("{program} built on {}", host.name))?;
        }
        layout::stamp(&kept, commit)?;
        fs::create_dir_all(into)?;
        fs::copy(kept.join(BUILT_FROM), into.join(BUILT_FROM))?;
        Ok(())
    }

    fn pull_from(hosts: &Path) -> impl FnOnce(&str, &str, &Path) -> Result<(), String> + '_ {
        move |ssh, commit, runtime| {
            fs::create_dir_all(runtime).map_err(|error| error.to_string())?;
            for file in PROGRAMS.iter().chain([&BUILT_FROM]) {
                fs::copy(hosts.join(ssh).join(commit).join(file), runtime.join(file)).map_err(|error| error.to_string())?;
            }
            Ok(())
        }
    }

    fn artifact(layout: &Layout, hosts: &Path, tree: &str, commit: &str) -> PathBuf {
        let artifact = layout.artifacts().join(tree);
        held_on_host(&host("cpx62", None), commit, &artifact, &mut |host, commit, into, stop| build_there(hosts, host, commit, into, stop), &|| false).unwrap();
        artifact
    }

    #[test]
    fn a_ready_tree_is_adopted_with_no_build_and_only_the_tree_its_commit_carries_matches() {
        let state = tempfile::tempdir().unwrap();
        let layout = Layout { state: state.path().to_path_buf() };
        let checkout = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| assert!(std::process::Command::new("git").current_dir(checkout.path()).args(args).status().unwrap().success());
        git(&["init", "-q"]);
        fs::write(checkout.path().join("a"), "a\n").unwrap();
        git(&["add", "a"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "a"]);
        let tree = tree_of(checkout.path(), "HEAD").unwrap();
        assert!(ready_for(&layout, checkout.path(), "HEAD").is_none(), "no artifact is read before one is kept");
        let head = String::from_utf8(std::process::Command::new("git").current_dir(checkout.path()).args(["rev-parse", "HEAD"]).output().unwrap().stdout).unwrap().trim().to_owned();
        let carried = |commit: &str| tree_of(checkout.path(), commit).ok();
        let hosts = tempfile::tempdir().unwrap();
        artifact(&layout, hosts.path(), &tree, &"f".repeat(40));
        let runtime = layout.runtime(&"a".repeat(40));
        assert!(!super::adopt(&layout, &runtime, &tree, &"a".repeat(40), carried, pull_from(hosts.path())), "an artifact whose built-from commit does not carry the landed tree is never installed, whichever lane asked for it");
        artifact(&layout, hosts.path(), &tree, &head);
        assert_eq!(ready_for(&layout, checkout.path(), "HEAD").as_deref(), Some(tree.as_str()), "the promotion finds the artifact by the tree its landed commit carries");
        assert!(super::adopt(&layout, &runtime, &tree, &"a".repeat(40), carried, pull_from(hosts.path())));
        assert!(layout::has_programs(&runtime) && layout::usable(&runtime, true), "the promotion pulls the build the artifact names into a whole runtime");
        assert!(pin::read(&layout.state).iter().any(|event| matches!(event, Event::ArtifactReady { sha, seconds: 0, .. } if sha == &"a".repeat(40))), "the promotion journals artifact-ready at 0 s");
        let journal = fs::read_to_string(layout.state.join(pin::JOURNAL)).unwrap();
        assert!(journal.contains("\"event\":\"artifact-ready\"") && journal.contains("\"seconds\":0"), "the ledger line is the one the box's jq reads");
        assert_eq!(layout::built_from_own_commit(&runtime), Ok(()), "the artifact's install is stamped for the landed commit");
        assert!(!super::adopt(&layout, &layout.runtime(&"b".repeat(40)), &"c".repeat(40), &"b".repeat(40), carried, pull_from(hosts.path())), "a tree with no artifact builds as before");
    }

    fn host(name: &str, free_cpus: Option<f64>) -> BuildHost {
        BuildHost { name: name.to_owned(), ssh: format!("tom@{name}"), free_cpus }
    }

    fn superseded_build_ends_at_once(layout: &Layout, hosts: &Path) {
        let artifacts = layout.artifacts();
        let ask = |tree: &str, commit: &str, kind, lane: &str| Ask { tree: tree.to_owned(), commit: commit.to_owned(), kind, lane: lane.to_owned() };
        let (running, running_commit, replacing) = ("7".repeat(40), "8".repeat(40), "9".repeat(40));
        let mut tried = Vec::new();
        let superseded_build = |host: &BuildHost, commit: &str, into: &Path, stop: Stop| -> Result<(), Failure> {
            tried.push(commit.to_owned());
            if commit != running_commit {
                return build_there(hosts, host, commit, into, stop);
            }
            file(&artifacts, &ask(&replacing, &replacing, Kind::HandIn, "i-lane")).unwrap();
            assert!(stop(), "a build asked for a lane's hand-in is told to stop once a newer request is filed");
            Err(failed(1, "stopped"))
        };
        request(layout, &ask(&running, &running_commit, Kind::HandIn, "h-lane"), || vec![host("gex45", Some(4.0)), host("cpx62", Some(12.0))], superseded_build).unwrap();
        assert_eq!(tried, [running_commit.clone(), replacing.clone()], "a superseded head's build ends at once and is not tried on the next build host, and the newer request builds after it");
        assert!(ready(layout, &running).is_none() && !staging(&artifacts, &running).exists() && ready(layout, &replacing).is_some(), "the ended build keeps nothing and the request that superseded it is kept");
    }

    #[test]
    fn build_ahead_never_runs_cargo_on_the_settling_host_and_copies_no_release_before_the_promotion() {
        let state = tempfile::tempdir().unwrap();
        let layout = Layout { state: state.path().to_path_buf() };
        let (tree, commit) = ("c".repeat(40), "d".repeat(40));
        let unbuilt = |_: &BuildHost, _: &str, _: &Path, _: Stop| -> Result<(), Failure> { panic!("no build host answered") };
        let ask = |tree: &str, commit: &str, kind, lane: &str| Ask { tree: tree.to_owned(), commit: commit.to_owned(), kind, lane: lane.to_owned() };
        assert!(request(&layout, &ask(&tree, &commit, Kind::Candidate, ""), || vec![host("cpx62", None), host("gex45", None)], unbuilt).is_ok(), "with no build host answering the request ends clean");
        let left: Vec<_> = fs::read_dir(layout.artifacts()).unwrap().flatten().map(|entry| entry.file_name()).collect();
        assert!(left.iter().all(|name| name == REQUESTS), "and it built nothing here: the promotion has a build host build the tree");
        assert!(filed(&layout.artifacts()).is_empty(), "a request that found no build host is drained, not left to block the next");
        file(&layout.artifacts(), &ask(&tree, &commit, Kind::Candidate, "")).unwrap();
        assert!(!awaited(&layout, &tree), "a request still queued is not waited for, and a promotion that builds the tree itself withdraws it");
        assert!(filed(&layout.artifacts()).is_empty(), "so no build ahead builds the tree a second time");

        let mut asked = Vec::new();
        let hosts = tempfile::tempdir().unwrap();
        let built = |host: &BuildHost, commit: &str, into: &Path, stop: Stop| -> Result<(), Failure> {
            asked.push(format!("{} {commit}", host.name));
            build_there(hosts.path(), host, commit, into, stop)
        };
        let artifacts = layout.artifacts();
        let (older, lane_old, lane_new, newer) = ("1".repeat(40), "2".repeat(40), "3".repeat(40), "4".repeat(40));
        file(&artifacts, &ask(&lane_old, &lane_old, Kind::HandIn, "f-lane")).unwrap();
        file(&artifacts, &ask(&older, &older, Kind::Candidate, "")).unwrap();
        file(&artifacts, &ask(&older, &older, Kind::HandIn, "g-lane")).unwrap();
        file(&artifacts, &ask(&lane_new, &lane_new, Kind::HandIn, "f-lane")).unwrap();
        assert_eq!(filed(&artifacts).len(), 2, "a lane's newer hand-in replaces its older one and a hand-in of a candidate's tree leaves it a candidate");
        file(&artifacts, &ask(&newer, &newer, Kind::Candidate, "")).unwrap();
        assert_eq!(filed(&artifacts).into_iter().map(|(tree, _)| tree).collect::<Vec<_>>(), [newer], "a candidate withdraws every older request");
        request(&layout, &ask(&tree, &commit, Kind::Candidate, ""), || vec![host("gex45", Some(4.0)), host("cpx62", Some(12.0))], built).unwrap();
        assert_eq!(asked, [format!("cpx62 {commit}")], "a newer candidate withdraws every older request still waiting, candidate or hand-in, so only the newest waiting tree is built and a superseded head's build never starts; a hand-in never withdraws a candidate");
        let artifact = ready(&layout, &tree).expect("the build host's release is kept by the candidate's tree");
        assert!(PROGRAMS.iter().all(|program| !artifact.join(program).exists()), "the landing copies no release north into runtime-artifacts: the build host keeps it until the promotion");
        let landed = "e".repeat(40);
        let runtime = layout.runtime(&landed);
        assert!(super::adopt(&layout, &runtime, &tree, &landed, |built: &str| (built == commit).then(|| tree.clone()), pull_from(hosts.path())), "the promotion of the landing obtains the release north from the build host that holds it");
        assert_eq!(fs::read_to_string(runtime.join("north")).unwrap(), "north built on cpx62");
        assert!(fs::read_to_string(runtime.join(BUILT_ON)).unwrap().starts_with("cpx62 ") && !runtime.join(layout::BUILT_HERE).exists(), "the promoted runtime says which host built it");
        assert_eq!(layout::built_from_own_commit(&runtime), Ok(()));

        let mut busy = None;
        assert!(north_base::platform::wait::until(std::time::Duration::from_secs(5), || { busy = slot(&artifacts, &host("cpx62", Some(12.0))); busy.is_some() }), "cpx62's slot is free once its drain ended, though a child another test forks keeps a copy of it until its exec");
        let busy = busy.unwrap();
        let (beside, beside_commit) = ("5".repeat(40), "6".repeat(40));
        let mut beside_on = Vec::new();
        let beside_built = |host: &BuildHost, commit: &str, into: &Path, stop: Stop| -> Result<(), Failure> {
            beside_on.push(host.name.clone());
            build_there(hosts.path(), host, commit, into, stop)
        };
        request(&layout, &ask(&beside, &beside_commit, Kind::HandIn, "h-lane"), || vec![host("gex45", Some(4.0)), host("cpx62", Some(12.0))], beside_built).unwrap();
        drop(busy);
        assert_eq!(beside_on, ["gex45"], "a request filed while a build ahead holds one build host's slot builds beside it on another build host, never queued behind it");

        superseded_build_ends_at_once(&layout, hosts.path());
    }
}
