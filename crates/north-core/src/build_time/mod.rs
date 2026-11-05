mod checkout;
mod config;
mod issue;
mod ledger;
mod measure;
mod threshold;
mod timings;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use config::Workspace;
use ledger::{Row, Verdict};
use measure::{Cargo, RealCargo};

use crate::platform::rolling_issue::{Gh, Tracker};

const USAGE: &str = "usage: north code-health build-time --repo PATH [--workspace REL [--leaf REL --hub REL]] [--samples N] [--ledger FILE] [--rev REF] [--file]";

#[derive(Debug, Default)]
struct Options {
    repo: PathBuf,
    workspace: Option<String>,
    leaf: Option<String>,
    hub: Option<String>,
    samples: usize,
    ledger: Option<PathBuf>,
    rev: Option<String>,
    file: bool,
    env: BTreeMap<String, String>,
}

pub fn run(arguments: &[String]) -> u8 {
    let mut options = match parse(arguments) {
        Ok(options) => options,
        Err(error) => return fail(&error),
    };
    options.env = std::env::vars().collect();
    let ledger = options.ledger.clone().unwrap_or_else(default_ledger);
    let host = crate::platform::host::name();
    let mut tracker = match options.file.then(|| issue::github_repo(&options.repo)).transpose() {
        Ok(Some(repo)) if repo != issue::REPO => return fail(&format!("--file ticks {}#{}, so it measures only {}; origin is {repo}", issue::REPO, issue::SPLIT_ISSUE, issue::REPO)),
        Ok(repo) => repo.map(|_| Gh::north()),
        Err(error) => return fail(&error),
    };
    let checkout = match options.rev.as_deref().map(|rev| checkout::Checkout::detached(&options.repo, rev)).transpose() {
        Ok(checkout) => checkout,
        Err(error) => return fail(&error),
    };
    if let Some(checkout) = &checkout {
        options.repo = checkout.path().to_path_buf();
    }
    match measure_repo(&options, &mut RealCargo, &ledger, &host, crate::platform::host::contention(), tracker.as_mut().map(|gh| gh as &mut dyn Tracker)) {
        Ok(code) => code,
        Err(error) => fail(&error),
    }
}

fn fail(message: &str) -> u8 {
    eprintln!("north code-health build-time: {message}");
    2
}

fn default_ledger() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    state.join("north/build-times.tsv")
}

fn parse(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options { samples: 3, ..Options::default() };
    let mut words = arguments.iter();
    let mut repo = None;
    while let Some(flag) = words.next() {
        let mut value = || words.next().cloned().ok_or(format!("{flag} needs a value\n{USAGE}"));
        match flag.as_str() {
            "--repo" => repo = Some(PathBuf::from(value()?)),
            "--workspace" => options.workspace = Some(value()?),
            "--leaf" => options.leaf = Some(value()?),
            "--hub" => options.hub = Some(value()?),
            "--samples" => options.samples = value()?.parse().map_err(|_| format!("--samples needs a positive count\n{USAGE}"))?,
            "--ledger" => options.ledger = Some(PathBuf::from(value()?)),
            "--rev" => options.rev = Some(value()?),
            "--file" => options.file = true,
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    options.repo = repo.ok_or(USAGE)?;
    if options.samples == 0 || ((options.leaf.is_some() || options.hub.is_some()) && options.workspace.is_none()) || (options.file && options.workspace.is_some()) {
        return Err(USAGE.to_owned());
    }
    Ok(options)
}

fn workspaces(options: &Options) -> Result<Vec<Workspace>, String> {
    let configured = config::read(&options.repo)?;
    let Some(name) = &options.workspace else {
        return if configured.is_empty() { Err(format!("{} has no [[build_time]] in structure.toml; pass --workspace, --leaf and --hub", options.repo.display())) } else { Ok(configured) };
    };
    let found = configured.into_iter().find(|entry| &entry.workspace == name);
    let leaf = options.leaf.clone().or_else(|| found.as_ref().map(|entry| entry.leaf.clone()));
    let hub = options.hub.clone().or_else(|| found.as_ref().map(|entry| entry.hub.clone()));
    let (Some(leaf), Some(hub)) = (leaf, hub) else { return Err(format!("workspace {name} is not in structure.toml [[build_time]]; pass --leaf and --hub")) };
    Ok(vec![Workspace { workspace: name.clone(), leaf, hub, threshold_seconds: found.as_ref().and_then(|entry| entry.threshold_seconds), contention_ceiling: found.and_then(|entry| entry.contention_ceiling) }])
}

fn measure_repo(options: &Options, cargo: &mut dyn Cargo, ledger: &Path, host: &str, contention: Option<f64>, tracker: Option<&mut dyn Tracker>) -> Result<u8, String> {
    let sha = git(&options.repo, &["rev-parse", "--short=10", "HEAD"]).unwrap_or_else(|| "unknown".to_owned());
    let repo_name = options.repo.canonicalize().ok().and_then(|path| repo_label(&path)).unwrap_or_else(|| "unknown".to_owned());
    let rustc = tool_version("rustc");
    let recorded_at = utc_now();
    let mut over = false;
    let mut boxes = Vec::new();
    for entry in workspaces(options)? {
        let root = options.repo.join(&entry.workspace);
        let times = measure::measure(cargo, &root, &root.join(&entry.leaf), &root.join(&entry.hub), options.samples)?;
        let (limit, warning) = threshold::limit(&entry.workspace, entry.threshold_seconds);
        if let Some(warning) = warning {
            eprintln!("north code-health build-time: {warning}");
        }
        let build = config::build(&root, &options.env);
        let verdict = Verdict::judge(times.leaf_test, limit, contention, entry.contention_ceiling);
        let row = Row { recorded_at: &recorded_at, host, repo: &repo_name, workspace: &entry.workspace, sha: &sha, rustc: &rustc, samples: options.samples, times: &times, threshold: limit, verdict, contention, build: &build };
        ledger::append(ledger, &row)?;
        println!("{}", report(&row));
        over |= verdict == Verdict::Over;
        boxes.extend(issue::split_box(&row, &entry.leaf));
    }
    println!("recorded in {}", ledger.display());
    if let Some(tracker) = tracker {
        issue::file(tracker, &boxes)?;
    }
    Ok(u8::from(over))
}

fn report(row: &Row<'_>) -> String {
    let t = row.times;
    let load = row.contention.map_or_else(String::new, |load| format!(", host load {load:.2} per thread"));
    let limit = format!("threshold {}s", row.threshold);
    format!(
        "{} {} at {} on {}: cold build {:.1}s; leaf edit build {:.1}s, test --no-run {:.1}s; hub edit build {:.1}s, test --no-run {:.1}s; {limit}: {}{load}\n  incremental {}, debuginfo {}\n  critical path {}",
        row.repo, row.workspace, row.sha, row.host, t.cold_build, t.leaf_build, t.leaf_test, t.hub_build, t.hub_test, row.verdict.word(), row.build.incremental.shown(), row.build.debuginfo.shown(), t.critical_path
    )
}

fn repo_label(path: &Path) -> Option<String> {
    let top = git(path, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).map(PathBuf::from);
    let top = top.and_then(|common| common.parent().map(Path::to_path_buf)).unwrap_or_else(|| path.to_path_buf());
    let name = |path: &Path| path.file_name().map(|name| name.to_string_lossy().into_owned());
    match name(&top)?.as_str() {
        "main" => top.parent().and_then(name),
        other => Some(other.to_owned()),
    }
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").arg("-C").arg(repo).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|text| !text.is_empty())
}

fn tool_version(tool: &str) -> String {
    std::process::Command::new(tool)
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| String::from_utf8_lossy(&output.stdout).split_whitespace().nth(1).map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn utc_now() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", now.year(), u8::from(now.month()), now.day(), now.hour(), now.minute(), now.second())
}
