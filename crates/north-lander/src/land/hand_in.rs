//! `north hand-in` and `north finish`: the calls a session hands its lane in with. Each stages and commits exactly the named paths in the lane that holds them, refusing a path in another checkout or paths with no changes before anything is committed, lands the lane's earlier commits whole, printing which paths they change that the call did not name and refusing only an unnamed deletion among them, and rebases a lane behind its destination onto it, with its uncommitted changes, when that applies without a conflict (north#1239), then hands the lane to the road `safe-push --to main` takes, so every gate comes back unchanged. `hand-in` waits there for the lander's verdict (north#1158); `finish` first runs the session's acceptance command, commits and queues only when it passes, running it as the check before queueing on the would-be head's tree, keyed and recorded as the lander's own run, when the lane's checks before queueing hold that command, so the safe-push it hands the lane to reuses that run (north#1230), and returns at the ticket (north#1194); given `--files`, it first writes into the session's own lane, the checkout holding its directory, each file's whole content or, for a file that exists, its replacements of old text that occurs exactly once, refusing a path outside the lane, an entry carrying both forms, or an absent or repeated old text before anything is written (north#1218). `finish --judge-message` scans a launch's proposed message, keeps it as the lane's proposal for the launched session once the scan passes and asks its private-kind verdict, and a `finish` whose message is empty commits with that proposal only when it is its own session's; any launch into the lane clears the proposal it finds (north#1239).

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use super::enqueue::{default_branch, fetch_base, git};
use crate::land::slots::runner;
use north_base::platform::finish_record::{FinishRecords, Finished};
use crate::land::scans::outgoing::denylist::{Denylist, Stand};

const EARLY_WAIT: std::time::Duration = std::time::Duration::from_secs(60);
const ATTEMPT: &str = "north-attempt.json";

pub const USAGE: &str = "usage: north hand-in --message TEXT [--] PATH...";
pub const FINISH_USAGE: &str = "usage: north finish --check COMMAND --message TEXT [--files JSON|-] [--] PATH... | north finish --judge-message SESSION < MESSAGE";
const JUDGE_MESSAGE: &str = "--judge-message";
const PROPOSAL_FILE: &str = "north-proposed-message";
const PROPOSALS: &str = "north/land/proposed-messages.jsonl";
pub const PROPOSED: &str = "proposed";
pub const REFUSED: &str = "refused";

pub struct Lane {
    pub root: PathBuf,
    pub destination: String,
}

/// Where a handed-in lane goes once its paths are committed. It may refuse a lane before anything is staged, as a worker sandbox refuses a lane whose git lock files it could create but not remove (north#1172).
pub trait Road {
    fn writable(&self, _root: &Path) -> Result<(), String> {
        Ok(())
    }

    fn failed(&self, _root: &Path, _failure: &str) {}

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
    content: Option<String>,
    replacements: Option<Vec<Replacement>>,
}

#[derive(serde::Deserialize)]
struct Replacement {
    old: String,
    new: String,
}

struct Request<'a> {
    check: Option<&'a str>,
    message: &'a str,
    paths: Vec<PathBuf>,
    files: Vec<File>,
}

struct Named {
    lane: Lane,
    message: String,
    paths: Vec<String>,
    targets: Vec<PathBuf>,
    committed: bool,
    added: Vec<String>,
}

pub fn run(arguments: &[String], road: impl Road) -> u8 {
    let request = match arguments {
        [flag, message, rest @ ..] if flag == "--message" => Request { check: None, message, paths: paths(rest), files: Vec::new() },
        _ => return usage(arguments, USAGE),
    };
    hand(&request, "hand-in", road)
}

pub fn finish(arguments: &[String], road: impl Road) -> u8 {
    if let [flag, session] = arguments
        && flag == JUDGE_MESSAGE
    {
        return judge_message(session);
    }
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
    serde_json::from_str(&text).map_err(|error| format!("the files in {source} are not a JSON list of {{\"path\", \"content\" or \"replacements\": [{{\"old\", \"new\"}}]}}: {error}"))
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
    if let Some(rebased) = rebase_onto_destination(&named.lane) {
        println!("{verb}: {rebased}");
    }
    if !named.added.is_empty() {
        println!("{verb}: also landing {}, which the lane's commits change and the call did not name", named.added.join(" "));
    }
    let head = request.check.and_then(|_| would_be(&named.lane.root, &named.message, &named.paths));
    if let Some(head) = &head {
        ask_early(&named, head.clone());
    }
    let landed = request.check.zip(head.as_deref()).and_then(|(check, head)| as_land_check(&named.lane, check, head));
    let checked = match (request.check, landed) {
        (_, Some(ran)) => ran.map_err(|refusal| ("failed as a check before queueing".to_owned(), refusal)),
        (Some(check), None) => passes(&named.lane.root, check),
        (None, None) => Ok(()),
    };
    if let Some(check) = request.check {
        record_finished(check, head.as_deref(), checked.is_ok());
    }
    if let (Some(check), Err((failure, output))) = (request.check, checked) {
        eprint!("{output}");
        road.failed(&named.lane.root, &failure);
        let written = if request.files.is_empty() { "" } else { "the files stay as written; " };
        let attempt = keep_attempt(&named, head.as_deref(), &output).map_or_else(|why| format!("the attempt was not kept: {why}"), |path| format!("the attempt is kept in {}", path.display()));
        eprintln!("{verb}: ✗ the check `{check}` {failure}; {written}NOT committed, NOT queued; {attempt}");
        return 1;
    }
    if request.check.is_some() {
        forget_attempt(&named.lane.root);
    }
    match commit(&named) {
        Ok(()) => road.take(&named.lane),
        Err(refusal) => {
            eprintln!("{verb}: ✗ {refusal}; NOT committed.");
            1
        }
    }
}

fn rebase_onto_destination(lane: &Lane) -> Option<String> {
    let root = &lane.root;
    let (remote_ref, base) = fetch_base(root, &lane.destination).ok()?;
    let behind = git(root, &["merge-base", "--is-ancestor", &base, "HEAD"]).is_err();
    let published = git(root, &["rev-parse", "--symbolic-full-name", "@{upstream}"]).is_ok_and(|upstream| upstream != remote_ref);
    if !behind || published {
        return None;
    }
    let replayed = git(root, &["replay", "--ref-action=print", "--onto", &base, &format!("{remote_ref}..HEAD")]).ok()?;
    let rebased = replayed.split_whitespace().nth(2).map_or_else(|| base.clone(), str::to_owned);
    let uncommitted = git(root, &["stash", "create"]).ok()?;
    if !uncommitted.is_empty() {
        git(root, &["merge-tree", "--write-tree", "--merge-base", "HEAD", &rebased, &uncommitted]).ok()?;
    }
    let commits = git(root, &["rev-list", "--count", &format!("{remote_ref}..HEAD")]).ok()?;
    if git(root, &["rebase", "--quiet", "--autostash", &base]).is_err() {
        let _ = git(root, &["rebase", "--abort"]);
        return None;
    }
    Some(format!("rebased the lane's {commits} commit(s) and its uncommitted changes onto origin/{} at {}, which moved on without a conflict", lane.destination, &base[..base.len().min(10)]))
}

fn as_land_check(lane: &Lane, check: &str, head: &str) -> Option<Result<(), String>> {
    git(&lane.root, &["show", &format!("{head}:.safe-push")]).ok().filter(|config| config.contains(check.trim()))?;
    let (_, base) = fetch_base(&lane.root, &lane.destination).ok()?;
    let commands = runner::queue_commands(&lane.root, &base, head).ok()?;
    commands.iter().any(|command| command.trim() == check.trim()).then(|| runner::refuse_queue_checks(&lane.root, &base, head))
}

fn passes(root: &Path, check: &str) -> Result<(), (String, String)> {
    if check.trim().is_empty() {
        return Err(("is empty".to_owned(), String::new()));
    }
    let unstarted = |error: io::Error| (format!("did not start: {error}"), String::new());
    let mut log = tempfile::tempfile().map_err(unstarted)?;
    let (out, err) = (log.try_clone().map_err(unstarted)?, log.try_clone().map_err(unstarted)?);
    let status = Command::new("sh").arg("-c").arg(check).current_dir(root).stdin(Stdio::null()).stdout(out).stderr(err).status().map_err(unstarted)?;
    let mut output = String::new();
    let _ = io::Seek::rewind(&mut log).and_then(|()| io::Read::read_to_string(&mut log, &mut output));
    match status.code() {
        Some(0) => {
            eprint!("{output}");
            Ok(())
        }
        Some(code) => Err((format!("exited {code}"), output)),
        None => Err(("was killed by a signal".to_owned(), output)),
    }
}

fn record_finished(check: &str, head: Option<&str>, passed: bool) {
    let Some(session) = std::env::var("NORTH_FLEET_SESSION").ok().filter(|session| !session.trim().is_empty()) else { return };
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let finished = Finished { session, command: check.trim().to_owned(), head: head.unwrap_or_default().to_owned(), passed, at };
    if let Err(error) = FinishRecords::here().record(&finished) {
        eprintln!("finish: the finish record was not written: {error}");
    }
}

fn forget_attempt(root: &Path) {
    if let Ok(path) = git(root, &["rev-parse", "--path-format=absolute", "--git-path", ATTEMPT]) {
        let _ = fs::remove_file(path);
    }
}

fn keep_attempt(named: &Named, head: Option<&str>, output: &str) -> Result<PathBuf, String> {
    let root = &named.lane.root;
    let path = PathBuf::from(git(root, &["rev-parse", "--path-format=absolute", "--git-path", ATTEMPT])?);
    let base = format!("refs/remotes/origin/{}", named.lane.destination);
    let diff = head.map_or_else(|| git(root, &["diff", &base]), |head| git(root, &["diff", &base, head]))?;
    fs::write(&path, serde_json::json!({"output": output, "diff": diff}).to_string()).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path)
}

fn name(directory: &Path, request: &Request, road: &impl Road) -> Result<Named, String> {
    let targets: Vec<PathBuf> = request.files.iter().map(|file| normal(&directory.join(&file.path))).collect();
    let paths: Vec<PathBuf> = request.paths.iter().map(|path| normal(&directory.join(path))).chain(targets.iter().cloned()).collect();
    let Some(first) = paths.first() else { return Err("name the paths to hand in".to_owned()) };
    let home = if targets.is_empty() { first.as_path() } else { directory };
    let root = checkout(home).ok_or_else(|| format!("{} is in no git checkout", home.display()))?;
    if let Some(outside) = paths.iter().find(|path| checkout(path).as_ref() != Some(&root)) {
        return Err(format!("{} is outside the lane {}", outside.display(), root.display()));
    }
    road.writable(&root)?;
    let message = match (request.message.trim().is_empty(), request.check) {
        (false, _) => request.message.to_owned(),
        (true, Some(_)) => proposed_message(&root).ok_or("the message is empty and the brief proposed none")?,
        (true, None) => return Err("the message is empty".to_owned()),
    };
    if request.check.is_some() && proposed_message(&root).is_some_and(|proposed| proposed == message) {
        record_proposal(&root, "used", &message);
    }
    let destination = default_branch(&root);
    match git(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Ok(branch) if branch != destination => {}
        _ => return Err(format!("{} is not on a lane's own branch", root.display())),
    }
    let named: Vec<String> = paths.iter().map(|path| path.to_str().map(str::to_owned).ok_or_else(|| "a path is not valid UTF-8".to_owned())).collect::<Result<_, _>>()?;
    let relative: Vec<String> = paths.iter().map(|path| relative_to(&root, path).ok_or_else(|| format!("{} is outside the lane {}", path.display(), root.display()))).collect::<Result<_, _>>()?;
    let committed = lane_changes(&root, &destination)?;
    if let Some((path, _)) = paths.iter().zip(&relative).find(|(path, name)| !path.exists() && !targets.contains(path) && !in_lane(&root, name, &committed)) {
        return Err(format!("{} is not in the lane: it is in neither the lane's tree nor its base, so there is no change to stage", path.display()));
    }
    let unnamed: Vec<String> = committed.iter().filter(|change| !relative.iter().any(|path| path == *change)).cloned().collect();
    let deleted = git(&root, &["diff", "--name-only", "--diff-filter=D", &format!("refs/remotes/origin/{destination}...HEAD"), "--"])?;
    let unnamed_deletions: Vec<&str> = unnamed.iter().map(String::as_str).filter(|change| deleted.lines().any(|line| line == *change)).collect();
    if !unnamed_deletions.is_empty() {
        return Err(format!("the lane's commits also delete {}, which the hand-in did not name", unnamed_deletions.join(" ")));
    }
    Ok(Named { lane: Lane { root, destination }, message, paths: named, targets, committed: !committed.is_empty(), added: unnamed })
}

fn apply(named: Named, files: &[File]) -> Result<Named, String> {
    let mut written: Vec<(&PathBuf, String)> = Vec::new();
    for (target, file) in named.targets.iter().zip(files) {
        let earlier = written.iter().rev().find(|(path, _)| *path == target).map(|(_, text)| text.as_str());
        let content = changed(target, file, earlier)?;
        written.push((target, content));
    }
    for (target, content) in &written {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        fs::write(target, content).map_err(|error| format!("{}: {error}", target.display()))?;
    }
    let paths: Vec<&str> = named.paths.iter().map(String::as_str).collect();
    if !named.committed && literal(&named.lane.root, &[&["status", "--porcelain", "--untracked-files=all", "--"][..], &paths].concat())?.is_empty() {
        return Err(format!("no changes in {}", paths.join(" ")));
    }
    Ok(named)
}

fn changed(target: &Path, file: &File, earlier: Option<&str>) -> Result<String, String> {
    let replacements = match (&file.content, &file.replacements) {
        (Some(content), None) => return Ok(content.to_owned()),
        (None, Some(replacements)) => replacements,
        (Some(_), Some(_)) => return Err(format!("{}: carries both whole content and replacements; send one; NOT written", target.display())),
        (None, None) => return Err(format!("{}: carries neither content nor replacements; NOT written", target.display())),
    };
    let mut text = match earlier {
        Some(text) => text.to_owned(),
        None => fs::read_to_string(target).map_err(|error| format!("{}: {error}; send a new file's whole content", target.display()))?,
    };
    for (number, replacement) in replacements.iter().enumerate().map(|(index, replacement)| (index + 1, replacement)) {
        let found = if replacement.old.is_empty() { 0 } else { text.matches(replacement.old.as_str()).count() };
        match found {
            1 => text = text.replacen(replacement.old.as_str(), &replacement.new, 1),
            0 => return Err(format!("{}: the old text of replacement {number} is absent; NOT written", target.display())),
            many => return Err(format!("{}: the old text of replacement {number} occurs {many} times; NOT written", target.display())),
        }
    }
    Ok(text)
}

fn commit(named: &Named) -> Result<(), String> {
    let message = named.message.as_str();
    let (root, paths) = (&named.lane.root, named.paths.iter().map(String::as_str).collect::<Vec<_>>());
    literal(root, &[&["add", "--all", "--"][..], &paths].concat())?;
    if literal(root, &[&["diff", "--cached", "--name-only", "HEAD", "--"][..], &paths].concat())?.is_empty() {
        return if named.committed { Ok(()) } else { Err(format!("no changes in {}", paths.join(" "))) };
    }
    literal(root, &[&["commit", "--quiet", "--only", "--message", message, "--"][..], &paths].concat()).map(drop)
}

fn literal(root: &Path, arguments: &[&str]) -> Result<String, String> {
    git(root, &[&["--literal-pathspecs"][..], arguments].concat())
}

fn in_lane(root: &Path, relative: &str, committed: &[String]) -> bool {
    let under = format!("{}/", relative.trim_end_matches('/'));
    let known = |arguments: &[&str]| literal(root, &[arguments, &["--", relative]].concat()).is_ok_and(|names| !names.is_empty());
    committed.iter().any(|change| change == relative || change.starts_with(&under)) || known(&["ls-files", "--cached"]) || known(&["ls-tree", "-r", "--name-only", "HEAD"])
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

fn ask_early(named: &Named, head: String) {
    let root = named.lane.root.clone();
    let destination = named.lane.destination.clone();
    std::thread::spawn(move || {
        let Ok(origin) = git(&root, &["remote", "get-url", "--push", "origin"]) else { return };
        let base = format!("refs/remotes/origin/{destination}");
        let _ = Denylist::from_env().verdict_within(&root, &origin, (&base, &head, &destination), Some(EARLY_WAIT));
    });
}

fn judge_message(session: &str) -> u8 {
    let message = io::read_to_string(io::stdin()).unwrap_or_default();
    let Some(root) = std::env::current_dir().ok().and_then(|directory| checkout(&directory)) else { return 1 };
    let Some(kept) = proposal_path(&root) else { return 1 };
    let _ = fs::remove_file(&kept);
    let destination = default_branch(&root);
    if message.trim().is_empty() || session.trim().is_empty() || session.contains('\n') || !git(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"]).is_ok_and(|branch| branch != destination) {
        return 1;
    }
    let (Ok(origin), Ok(base)) = (git(&root, &["remote", "get-url", "--push", "origin"]), git(&root, &["rev-parse", "HEAD"])) else { return 1 };
    let Some(head) = message_only(&root, &base, &message) else { return 1 };
    let passed = std::cell::Cell::new(false);
    let on_pass = || {
        if fs::write(&kept, format!("{session}\n{message}")).is_ok() {
            println!("{PROPOSED}");
            passed.set(true);
            record_proposal(&root, "judged", &message);
        }
        let _ = io::Write::flush(&mut io::stdout());
    };
    let verdict = Denylist::from_env().verdict_once_scanned(&root, &origin, (&base, &head, &destination), EARLY_WAIT, &on_pass);
    if verdict.stand == Stand::Refuse && !passed.get() {
        println!("{REFUSED}");
    }
    u8::from(verdict.stand != Stand::Pass)
}

fn proposal_path(root: &Path) -> Option<PathBuf> {
    git(root, &["rev-parse", "--path-format=absolute", "--git-path", PROPOSAL_FILE]).ok().map(PathBuf::from)
}

fn proposed_message(root: &Path) -> Option<String> {
    let own = std::env::var("NORTH_FLEET_SESSION").ok().filter(|session| !session.trim().is_empty())?;
    let kept = fs::read_to_string(proposal_path(root)?).ok()?;
    let (session, message) = kept.split_once('\n')?;
    Some(message.to_owned()).filter(|message| session == own && !message.trim().is_empty())
}

fn record_proposal(root: &Path, event: &str, message: &str) {
    let digest: String = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(message.as_bytes())).iter().map(|byte| format!("{byte:02x}")).collect();
    let line = serde_json::json!({"at": north_base::platform::records_mirror::now(), "event": event, "lane": root, "message": digest});
    let journal = north_base::platform::records_mirror::state_home().join(PROPOSALS);
    let appended = journal.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::OpenOptions::new().create(true).append(true).open(&journal)).and_then(|mut file| io::Write::write_all(&mut file, format!("{line}\n").as_bytes()));
    if let Err(error) = appended {
        eprintln!("finish: the proposed message's {event} was not recorded in {}: {error}", journal.display());
    }
}

fn message_only(root: &Path, base: &str, message: &str) -> Option<String> {
    let cleaned = piped(root, None, &["stripspace"], Some(message))?;
    piped(root, None, &["commit-tree", &format!("{base}^{{tree}}"), "-p", base, "-F", "-"], Some(&cleaned)).map(|head| head.trim().to_owned())
}

fn would_be(root: &Path, message: &str, paths: &[String]) -> Option<String> {
    let index = PathBuf::from(git(root, &["rev-parse", "--path-format=absolute", "--git-path", "index"]).ok()?);
    let scratch = index.with_file_name(format!("index.finish-{}", std::process::id()));
    let staged = |arguments: &[&str], input: Option<&str>| piped(root, Some(&scratch), arguments, input);
    let made = (|| {
        staged(&["read-tree", "HEAD"], None)?;
        staged(&[&["--literal-pathspecs", "add", "--all", "--"][..], &paths.iter().map(String::as_str).collect::<Vec<_>>()].concat(), None)?;
        let tree = staged(&["write-tree"], None)?;
        let cleaned = staged(&["stripspace"], Some(message))?;
        staged(&["commit-tree", tree.trim(), "-p", "HEAD", "-F", "-"], Some(&cleaned))
    })();
    let _ = std::fs::remove_file(&scratch);
    made.map(|head| head.trim().to_owned())
}

fn piped(root: &Path, index: Option<&Path>, arguments: &[&str], input: Option<&str>) -> Option<String> {
    let mut command = Command::new("git");
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let mut child = command.arg("-C").arg(root).args(arguments).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    std::io::Write::write_all(&mut stdin, input.unwrap_or_default().as_bytes()).ok()?;
    drop(stdin);
    let output = child.wait_with_output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
