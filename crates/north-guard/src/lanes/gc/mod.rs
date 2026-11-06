mod audit;
mod audit_rows;
mod evidence;
mod exec;
mod liveness;
mod local;
mod remote;
mod remote_boxes;
mod report;
mod scratch;
mod state;
mod strays;
mod targets;
mod trees;

#[cfg(test)]
mod strays_tests;
#[cfg(test)]
mod scratch_tests;
#[cfg(test)]
mod targets_tests;

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::live_process::open_paths;
use north_base::foundation::clock::Timestamp;
use north_base::platform::host;
use evidence::{IssueState, origin_repo};
use liveness::{GithubFacts, Hosts};
use state::{StaleReport, write_atomically};

pub use audit::run as repo;

const SKIP: [&str; 3] = ["clients", "resources", "archive"];
pub const USAGE: &str = include_str!("usage.txt").trim_ascii_end();
const REMOTE_RECORD: &str = "remote.txt";

#[derive(Clone, Debug)]
pub(super) struct Options {
    remote: bool,
    dry_run: bool,
    root: PathBuf,
    repos: Vec<String>,
    idle_hours: f64,
    state: PathBuf,
}

#[derive(Debug, Default)]
pub(super) struct Counts {
    retired_worktrees: usize,
    deleted_branches: usize,
    deleted_remote: usize,
    kept: usize,
    unlanded: usize,
    dropped_targets: usize,
    dropped_bytes: u64,
}

pub(super) struct Gc {
    home: PathBuf,
    now_ms: i64,
    options: Options,
    stale: StaleReport,
    stale_path: PathBuf,
    rows: Vec<String>,
    counts: Counts,
    hosts: Hosts,
    fleet: Option<Vec<Value>>,
    claims: Option<HashSet<String>>,
    issues: HashMap<String, IssueState>,
    pushes: HashMap<String, Option<HashMap<String, i64>>>,
    github: HashMap<String, GithubFacts>,
    open: HashMap<String, Vec<Value>>,
    remote_rows: Vec<String>,
}

enum Parsed {
    Sweep(Options),
    Exit(u8),
}

fn now_ms() -> i64 {
    std::env::var("LANE_GC_NOW").ok().and_then(|at| Timestamp::parse_rfc3339(&at)).unwrap_or_else(Timestamp::now).unix_millis()
}

fn tilde(home: &Path, path: &Path) -> String {
    path.strip_prefix(home).map_or_else(|_| path.display().to_string(), |rest| if rest.as_os_str().is_empty() { "~".into() } else { format!("~/{}", rest.display()) })
}

fn parse(arguments: &[String], home: &Path) -> Parsed {
    let mut options = Options {
        remote: false,
        dry_run: false,
        root: home.join("code"),
        repos: Vec::new(),
        idle_hours: 3.0,
        state: home.join(".local/state/agents/lane-gc/unlanded.txt"),
    };
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let mut value = || words.next().cloned().unwrap_or_default();
        match word.as_str() {
            "--remote" => options.remote = true,
            "--dry-run" => options.dry_run = true,
            "--repo" => options.repos.push(value()),
            "--root" => options.root = PathBuf::from(value()),
            "--idle-hours" => options.idle_hours = value().parse().unwrap_or(f64::NAN),
            "--state" => options.state = PathBuf::from(value()),
            "--unlanded" => return Parsed::Exit(print_unlanded(home, &options.state)),
            "--scan-trees" => {
                let root = trees::expand(&value(), home);
                let at = value().parse().unwrap_or_else(|_| now_ms());
                println!("{}", serde_json::to_string(&trees::scan(&root, at)).unwrap_or_default());
                return Parsed::Exit(0);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Parsed::Exit(0);
            }
            _ => {
                eprintln!("{USAGE}");
                return Parsed::Exit(2);
            }
        }
    }
    Parsed::Sweep(options)
}

fn print_unlanded(home: &Path, state: &Path) -> u8 {
    match std::fs::read_to_string(state) {
        Ok(text) => {
            print!("{text}");
            let rows: Vec<String> = text.lines().map(str::to_string).collect();
            for line in report::class_summary(&rows) {
                println!("{line}");
            }
            0
        }
        Err(_) => {
            eprintln!("lane-gc: no report yet at {}; run lane-gc --dry-run", tilde(home, state));
            1
        }
    }
}

fn containers(root: &Path, only: &[String]) -> std::io::Result<Vec<PathBuf>> {
    let mut names: Vec<String> = std::fs::read_dir(root)?.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    Ok(names
        .into_iter()
        .filter(|name| !SKIP.contains(&name.as_str()) && !name.ends_with("-data") && (only.is_empty() || only.contains(name)))
        .map(|name| root.join(name))
        .filter(|container| container.join("main/.git").exists())
        .collect())
}

pub fn run(arguments: &[String]) -> u8 {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let _ = std::env::set_current_dir(&home);
    let options = match parse(arguments, &home) {
        Parsed::Sweep(options) => options,
        Parsed::Exit(code) => return code,
    };
    let containers = match containers(&options.root, &options.repos) {
        Ok(containers) => containers,
        Err(error) => {
            eprintln!("lane-gc: {}: {error}", options.root.display());
            return 1;
        }
    };
    let now_ms = now_ms();
    let stale_path = std::env::var_os("LANE_GC_STALE_FILE").filter(|value| !value.is_empty()).map_or_else(|| home.join(".local/state/agents/lane-gc/stale.json"), PathBuf::from);
    let mut stale = StaleReport::load(&stale_path);
    stale.notices = Vec::new();
    stale.at = Timestamp::from_unix_millis(now_ms).iso_millis();
    let local = trees::scan(&options.root, now_ms);
    let on_nexus = host::name() == "nexus";
    let hosts = if on_nexus { Hosts { laptop: Default::default(), nexus: local, on_nexus } } else { Hosts { laptop: local, nexus: trees::scan_remote(now_ms), on_nexus } };
    let mut gc = Gc {
        home,
        now_ms,
        options,
        stale,
        stale_path,
        rows: Vec::new(),
        counts: Counts::default(),
        hosts,
        fleet: None,
        claims: None,
        issues: HashMap::new(),
        pushes: HashMap::new(),
        github: HashMap::new(),
        open: HashMap::new(),
        remote_rows: Vec::new(),
    };
    gc.sweep_all(&containers);
    gc.finish()
}

impl Gc {
    fn sweep_all(&mut self, containers: &[PathBuf]) {
        let live = if self.options.remote { Vec::new() } else { open_paths(Path::new("/proc")) };
        for container in containers {
            let name = container.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let has_origin = origin_repo(&container.join("main")).is_some();
            let local = if self.options.remote || !container.join("worktrees").exists() { Ok(()) } else { self.sweep(container, &live) };
            let swept = local.and_then(|()| if self.options.remote || has_origin { self.remote_sweep(container) } else { Ok(()) });
            if let Err(error) = swept {
                self.say(format!("{name}: stopped: {error}"));
            }
        }
        if !self.options.remote {
            if self.options.repos.is_empty() {
                self.drop_scratch_targets(&std::env::temp_dir(), &live);
            }
            self.raise_alerts();
        }
    }

    fn finish(&mut self) -> u8 {
        let stale = serde_json::to_string_pretty(&self.stale).unwrap_or_default() + "\n";
        if let Err(error) = write_atomically(&self.stale_path, &stale) {
            eprintln!("lane-gc: {}: {error}", self.stale_path.display());
            return 1;
        }
        let header = format!(
            "# lane-gc {}: unlanded worktrees and branches (age of last commit, container, path, branch, ahead, dirty, first subject, refs, class)\n",
            Timestamp::now().iso_millis()
        );
        let body = report::sorted(&self.rows).join("\n");
        let summary = report::class_summary(&self.rows).join("\n");
        if !self.options.dry_run && self.options.repos.is_empty() {
            let text = format!("{header}{body}{}", if body.is_empty() { "" } else { "\n" });
            if let Err(error) = write_atomically(&self.options.state, &text) {
                eprintln!("lane-gc: {}: {error}", self.options.state.display());
                return 1;
            }
            let remote = self.options.state.with_file_name(REMOTE_RECORD);
            let text = format!("# lane-gc {}: origin branch decisions\n{}\n", Timestamp::now().iso_millis(), self.remote_rows.join("\n"));
            if let Err(error) = write_atomically(&remote, &text) {
                eprintln!("lane-gc: {}: {error}", remote.display());
                return 1;
            }
        } else if self.options.dry_run {
            println!("{header}{body}");
            if !summary.is_empty() {
                println!("{summary}");
            }
        }
        let c = &self.counts;
        println!(
            "lane-gc{}: retired {} worktrees, {} local branches, {} origin branches; kept {} ({} unlanded); dropped {} build targets ({})",
            if self.options.dry_run { " (dry run)" } else { "" },
            c.retired_worktrees,
            c.deleted_branches,
            c.deleted_remote,
            c.kept,
            c.unlanded,
            c.dropped_targets,
            targets::gib(c.dropped_bytes)
        );
        0
    }

    fn notice(&mut self, repo: &str, branch: &str, action: &str) -> Result<(), String> {
        let owner = self.owner(repo)?;
        let line = format!("{} {repo} {branch} {action} owner={owner}", Timestamp::from_unix_millis(self.now_ms).iso_millis());
        self.stale.notices.push(line.clone());
        let path = self.stale_path.with_file_name("notices.md");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        OpenOptions::new().create(true).append(true).open(&path).and_then(|mut file| writeln!(file, "{line}")).map_err(|error| error.to_string())
    }

    fn tilde(&self, path: &Path) -> String {
        tilde(&self.home, path)
    }

    fn issue_states(&mut self, items: &[String]) -> Vec<IssueState> {
        items.iter().map(|item| *self.issues.entry(item.clone()).or_insert_with(|| evidence::issue_state(item))).collect()
    }
}
