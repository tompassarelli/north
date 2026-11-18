use std::ffi::OsStr;
use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::land::telemetry::epoch;
use super::ingest_gate::{self, Turn};
use super::route;
use north_base::platform::harness::{DEFAULTS, document};
use north_base::platform::{host, orchestration};
use crate::land::trace::{self, AFTER, Category, PHASE, Span};

const USAGE: &str = "usage: north land follow [--work] --repo DIR --git-dir DIR --base SHA --head SHA";

pub use north_base::platform::north_binary::land_north as program;

pub struct Follow<'a> {
    pub repo: &'a Path,
    pub git_dir: &'a Path,
    pub base: &'a str,
    pub head: &'a str,
    pub log: &'a Path,
}

pub fn spawn(follow: &Follow) -> Result<(), String> {
    if let Some(parent) = follow.log.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let log = File::create(follow.log).map_err(|error| format!("{}: {error}", follow.log.display()))?;
    let errors = log.try_clone().map_err(|error| error.to_string())?;
    let executable = std::env::current_exe().map_err(|error| format!("current_exe: {error}"))?;
    let mut child = Command::new(executable)
        .args(["land", "follow", "--repo"])
        .arg(follow.repo)
        .arg("--git-dir")
        .arg(follow.git_dir)
        .args(["--base", follow.base, "--head", follow.head])
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(errors)
        .env(trace::SPANS_ENV, follow.log.parent().map(|logs| trace::Spans::beside(logs).path().to_owned()).unwrap_or_default())
        .process_group(0)
        .spawn()
        .map_err(|error| format!("north land follow: {error}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn step(program: &str, arguments: &[&std::ffi::OsStr]) {
    println!("north land follow: {program} {}", arguments.iter().map(|argument| argument.to_string_lossy()).collect::<Vec<_>>().join(" "));
    match Command::new(program).args(arguments).stdin(Stdio::null()).status() {
        Ok(status) if status.success() => {}
        Ok(status) => println!("north land follow: {program} exited {status}"),
        Err(error) => println!("north land follow: {program}: {error}"),
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let mut values = std::collections::HashMap::new();
    let mut work = false;
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        if flag == "--work" {
            work = true;
            continue;
        }
        match (flag.as_str(), rest.next()) {
            ("--repo" | "--git-dir" | "--base" | "--head", Some(value)) => {
                values.insert(flag.as_str(), value.as_str());
            }
            _ => {
                eprintln!("{USAGE}");
                return 64;
            }
        }
    }
    let (Some(repo), Some(git_dir), Some(base), Some(head)) = (values.get("--repo"), values.get("--git-dir"), values.get("--base"), values.get("--head")) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let landed = Landed { repo: Path::new(repo), git_dir: Path::new(git_dir), base, head };
    let spans = if work {
        post_landing(&landed)
    } else {
        match placement(landed.repo) {
            Placement::Here => post_landing(&landed),
            Placement::On { host, login } => elsewhere(&landed, &host, &login),
            Placement::Unplaced(why) => {
                println!("north land follow: the post-landing work of {head} is skipped: {why}");
                Vec::new()
            }
        }
    };
    trace::record(&spans);
    0
}

struct Landed<'a> {
    repo: &'a Path,
    git_dir: &'a Path,
    base: &'a str,
    head: &'a str,
}

impl Landed<'_> {
    fn timed(&self, name: &str, category: Category, arguments: &[&OsStr]) -> Span {
        let start = epoch();
        step(&program(), arguments);
        Span::new(self.head, name, category, start, epoch()).on(&trace::this_host()).with(PHASE, AFTER)
    }
}

/// The post-landing work, which `post_landing_host` places: the runtime follow, the after-landing gates and the graph ingest.
fn post_landing(landed: &Landed) -> Vec<Span> {
    let os = OsStr::new;
    let mut spans = vec![
        landed.timed("runtime follow", Category::Follow, &[os("runtime"), os("follow"), os("--repo-dir"), landed.repo.as_os_str()]),
        landed.timed("after-landing gates", Category::Ci, &[os("after-landing"), os("--only"), os("gates"), os("--git-dir"), landed.git_dir.as_os_str(), os("--base"), os(landed.base), os("--head"), os(landed.head)]),
    ];
    match ingest_gate::take(landed.git_dir, landed.head, Duration::from_secs(north_base::platform::harness::default_number("graph_ingest_pause_secs") as u64)) {
        (Turn::Ingest { heads }, held) => {
            spans.push(landed.timed("graph ingest", Category::Follow, &[os("graph"), os("ingest")]));
            let commits: Vec<String> = heads.iter().map(|head| format!("crates@{head}")).collect();
            let mut arguments = vec![os("graph"), os("ingest")];
            arguments.extend(commits.iter().map(|commit| os(commit)));
            spans.push(landed.timed("graph ingest crates", Category::Follow, &arguments));
            held.into_iter().for_each(ingest_gate::Held::ended);
        }
        (Turn::Covered { by }, _) => println!("north land follow: graph ingest: {} is covered by the ingest of {by}, which waits and reads main as it stands when it starts, and tells the crate facts of {}", landed.head, landed.head),
    }
    spans
}

/// Runs the post-landing work on `host` in its own checkout of the repository, at the same path under its home, after fetching main there.
fn elsewhere(landed: &Landed, host: &str, login: &str) -> Vec<Span> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let Some(relative) = landed.repo.strip_prefix(&home).ok().and_then(Path::to_str) else {
        println!("north land follow: the post-landing work of {} is skipped: {} lies outside home, where no build host keeps it", landed.head, landed.repo.display());
        return Vec::new();
    };
    let script = format!(
        "{} cd \"$HOME\"/{} && git fetch -q --no-tags origin main && exec \"$n\" land follow --work --repo \"$PWD\" --git-dir \"$(git rev-parse --path-format=absolute --git-common-dir)\" --base {} --head {}",
        route::REMOTE_NORTH,
        route::quote(relative),
        route::quote(landed.base),
        route::quote(landed.head)
    );
    println!("north land follow: the post-landing work of {} runs on {host}", landed.head);
    let start = epoch();
    match Command::new("ssh").args(route::SSH_OPTIONS).args(["-T", login, "--", &script]).stdin(Stdio::null()).status() {
        Ok(status) if status.success() => {}
        Ok(status) => println!("north land follow: the post-landing work on {host} exited {status}"),
        Err(error) => println!("north land follow: ssh {login}: {error}"),
    }
    vec![Span::new(landed.head, "post-landing work", Category::Follow, start, epoch()).on(host).with(PHASE, AFTER)]
}

/// Writes the notice `message` for `session` to the lander's journal, where every notice goes since the fleet's messages were removed (north#1650); a caller that prints the line itself does not call this.
pub fn send(session: &str, message: &str) -> bool {
    println!("{message} (for {session})");
    true
}

const PLACEMENT: &str = "post_landing_host";
const ON_LANDER: &str = "lander";

/// Where a landing's post-landing work runs, the `post_landing_host` switch applied to this host.
enum Placement {
    Here,
    On { host: String, login: String },
    Unplaced(String),
}

/// Whether post-landing work runs on this host: the switch keeps it on the lander's host, or this host is a build host.
pub fn post_landing_here() -> bool {
    let name = host::name();
    document(DEFAULTS)[PLACEMENT]["value"].as_str() == Some(ON_LANDER) || route::fleet().is_some_and(|fleet| orchestration::build_hosts(&fleet).iter().any(|(build, _)| *build == name))
}

/// Here, on the build host with room under the operator login that reaches it, or nowhere when the switch names a build host and none answers.
fn placement(repo: &Path) -> Placement {
    if post_landing_here() {
        return Placement::Here;
    }
    let name = host::name();
    let Some(chosen) = build_host(repo, &name) else {
        return Placement::Unplaced(format!("no build host answered and {name} is no build host"));
    };
    match route::operator_login(&chosen) {
        Ok(login) => Placement::On { host: chosen, login },
        Err(why) => Placement::Unplaced(why),
    }
}

fn build_host(repo: &Path, here: &str) -> Option<String> {
    let chosen = Command::new(program()).args(["capacity", "offload", "choose", "--any", "--avoid", here]).current_dir(repo).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|output| output.status.success())?;
    let chosen = String::from_utf8_lossy(&chosen.stdout).trim().to_owned();
    (!chosen.is_empty() && chosen != here).then_some(chosen)
}
