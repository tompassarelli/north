use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use time::{Duration, OffsetDateTime};

use super::mutant::{Env, Stop, git_bytes, git_text, repo_name};
use super::mutant_retire::{self, Matrix};
use super::mutant_sites;
use super::mutant_trees::{self, Plan};

const EVERY_TEST: &str = "*";

pub trait Issues {
    fn body(&self, repo: &str, number: u64) -> Result<String, String>;
    fn set_body(&self, repo: &str, number: u64, body: &str) -> Result<(), String>;
    fn comment(&self, repo: &str, number: u64, body: &str) -> Result<String, String>;
}

pub(super) struct MatrixArgs {
    pub(super) mutants: usize,
    pub(super) seed: String,
    pub(super) timeout_seconds: u64,
}

pub(super) fn matrix(cwd: &Path, args: &MatrixArgs, env: &Env, out: &mut dyn Write, progress: &mut dyn Write) -> Result<(), Stop> {
    let repo = repo_name(cwd)?;
    let Some(config) = super::mutant_verify::config(cwd, "HEAD")? else {
        return Err(Stop::Usage("mutant matrix: .safe-push at HEAD has no [mutant] run".to_owned()));
    };
    if config.sources.is_empty() {
        return Err(Stop::Usage("mutant matrix: .safe-push at HEAD lists no [mutant] source pathspec".to_owned()));
    }
    let head = git_text(cwd, None, &["rev-parse", "HEAD"])?;
    let listed = git_bytes(cwd, None, &[&["ls-files", "-z", "--"][..], &config.sources.iter().map(String::as_str).collect::<Vec<_>>()].concat())?;
    let mut texts = BTreeMap::new();
    let mut all = Vec::new();
    for path in String::from_utf8_lossy(&listed).split('\0').filter(|path| !path.is_empty()) {
        let Ok(bytes) = git_bytes(cwd, None, &["show", &format!("HEAD:{path}")]) else { continue };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        all.extend(mutant_sites::sites(path, &text));
        texts.insert(path.to_owned(), text);
    }
    let found = all.len();
    let chosen = mutant_sites::sample(all, &args.seed, args.mutants);
    writeln!(progress, "mutant matrix: {} of {found} sites in {} files, seed {}", chosen.len(), texts.len(), args.seed)?;
    let scratch = tempfile::tempdir()?;
    let mut patches = BTreeMap::new();
    let mut labels = BTreeMap::new();
    for (index, site) in chosen.iter().enumerate() {
        let id = format!("{}-{index:03}", args.seed);
        let patch = scratch.path().join(format!("{id}.patch"));
        fs::write(&patch, mutant_sites::patch(site, &texts[&site.path]))?;
        patches.insert(id.clone(), patch);
        labels.insert(id, site.label());
    }
    let plan = Plan {
        head: &head,
        mutants: patches,
        tests: BTreeSet::from([EVERY_TEST.to_owned()]),
        config: &config,
        repo: &repo,
        timeout_seconds: args.timeout_seconds,
        trees_per_lease: 1,
    };
    let outcome = mutant_trees::run(cwd, &plan, env, progress)?;
    let mut result = Matrix { baseline: outcome.change, ..Matrix::default() };
    for (id, verdicts) in &outcome.mutants {
        result.mutants.insert(id.clone(), !verdicts.is_empty());
        let killed: BTreeSet<String> = verdicts.iter().filter(|(test, pass)| !**pass && result.baseline.get(*test) == Some(&true)).map(|(test, _)| test.clone()).collect();
        if !killed.is_empty() {
            result.kills.insert(id.clone(), killed);
        }
    }
    writeln!(progress, "mutant matrix: {} tests on the untouched tree, {:.0} s; runner output ends:\n{}", result.baseline.len(), outcome.seconds, outcome.tail)?;
    out.write_all(result.render(&labels).as_bytes())?;
    Ok(())
}

pub(super) struct RetireArgs {
    pub(super) repo: String,
    pub(super) matrices: Vec<PathBuf>,
    pub(super) evidence: PathBuf,
    pub(super) contracts: Vec<String>,
    pub(super) date: String,
    pub(super) days: i64,
}

pub(super) fn since(date: &str, days: i64) -> Result<String, Stop> {
    let bad = || Stop::Usage(format!("mutant: {date} is not YYYY-MM-DD"));
    let mut parts = date.splitn(3, '-').map(str::parse::<i32>);
    let (Some(Ok(year)), Some(Ok(month)), Some(Ok(day))) = (parts.next(), parts.next(), parts.next()) else { return Err(bad()) };
    let month = u8::try_from(month).ok().and_then(|month| time::Month::try_from(month).ok()).ok_or_else(bad)?;
    let day = time::Date::from_calendar_date(year, month, u8::try_from(day).map_err(|_| bad())?).map_err(|_| bad())?;
    Ok(iso(day - Duration::days(days)))
}

fn iso(day: time::Date) -> String {
    format!("{:04}-{:02}-{:02}", day.year(), u8::from(day.month()), day.day())
}

pub(super) fn retire(args: &RetireArgs) -> Result<String, Stop> {
    let tsv = fs::read_to_string(&args.evidence)
        .map_err(|error| Stop::Fail(format!("mutant retire: no catch evidence at {} ({error}); NOT proposing", args.evidence.display())))?;
    let evidence = mutant_retire::evidence(&tsv).map_err(|why| Stop::Fail(format!("mutant retire: {}: {why}", args.evidence.display())))?;
    let matrices = args.matrices.iter().map(|path| fs::read_to_string(path).map(|text| Matrix::parse(&text))).collect::<Result<Vec<_>, _>>()?;
    let since = since(&args.date, args.days)?;
    let verdict = mutant_retire::select(&matrices, &evidence, &args.contracts, &since);
    Ok(mutant_retire::render_box(&args.repo, &args.date, &since, &verdict))
}

pub(super) fn with_box(body: &str, line: &str) -> Option<String> {
    let key = line.split(':').next().unwrap_or(line);
    if body.lines().any(|existing| existing.starts_with(key)) {
        return None;
    }
    Some(format!("{}\n{line}\n", body.trim_end()))
}

const BODY_LIMIT: usize = 60_000;

fn chunks(list: &str) -> Vec<String> {
    let mut chunks = vec![String::new()];
    for item in list.split(", ") {
        let last = chunks.last_mut().expect("chunks starts non-empty");
        if !last.is_empty() && last.len() + item.len() + 2 > BODY_LIMIT {
            chunks.push(item.to_owned());
        } else {
            if !last.is_empty() {
                last.push_str(", ");
            }
            last.push_str(item);
        }
    }
    chunks
}

pub(super) fn post(issues: &dyn Issues, repo: &str, number: u64, line: &str, out: &mut dyn Write) -> Result<(), Stop> {
    let body = issues.body(repo, number).map_err(Stop::Fail)?;
    if with_box(&body, line).is_none() {
        writeln!(out, "mutant sweep: {repo}#{number} already carries this sweep's box")?;
        return Ok(());
    }
    let line = match line.split_once(mutant_retire::LIST_START) {
        Some((head, list)) if body.len() + line.len() >= BODY_LIMIT => {
            let parts = chunks(list);
            let mut urls = Vec::new();
            for (index, part) in parts.iter().enumerate() {
                let text = format!("{head} ({} of {}):\n\n{part}\n", index + 1, parts.len()).replacen("- [ ] ", "", 1);
                urls.push(issues.comment(repo, number, &text).map_err(Stop::Fail)?);
            }
            format!("{head}{}the list is in {}", mutant_retire::LIST_START, urls.join(", "))
        }
        _ => line.to_owned(),
    };
    if let Some(body) = with_box(&body, &line) {
        issues.set_body(repo, number, &body).map_err(Stop::Fail)?;
    }
    writeln!(out, "mutant sweep: added to {repo}#{number}: {line}")?;
    Ok(())
}

pub(super) struct SweepArgs {
    pub(super) repos: Vec<PathBuf>,
    pub(super) host: String,
    pub(super) mutants: usize,
    pub(super) timeout_seconds: u64,
    pub(super) days: i64,
    pub(super) issue: Option<(String, u64)>,
    pub(super) remote_north: String,
}

fn sweeps(env: &Env, repo: &str) -> PathBuf {
    env.mutants(repo).join("sweeps")
}

fn run_matrix(dir: &Path, args: &SweepArgs, seed: &str) -> Result<String, Stop> {
    let command = format!("{} mutant matrix --mutants {} --seed {seed} --timeout {}", args.remote_north, args.mutants, args.timeout_seconds);
    let output = Command::new(north_base::platform::north_binary::north_binary())
        .current_dir(dir)
        .args(["capacity", "offload", "run", "--host", &args.host, "--base", "HEAD", "--head", "HEAD", "--timeout", &(args.timeout_seconds * (args.mutants as u64 + 1) + 600).to_string(), "--", &command])
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let kept: String = text.lines().filter(|line| ["baseline\t", "mutant\t", "kill\t"].iter().any(|kind| line.starts_with(kind))).map(|line| format!("{line}\n")).collect();
    if !output.status.success() || !kept.lines().any(|line| line.starts_with("baseline\t")) {
        return Err(Stop::Fail(format!("mutant sweep: {} on {} returned {} with no baseline rows; stdout ends:\n{}", command, args.host, output.status, text.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"))));
    }
    Ok(kept)
}

fn sweep_repo(dir: &Path, args: &SweepArgs, env: &Env, date: &str, since: &str) -> Result<String, Stop> {
    let repo = repo_name(dir)?;
    let Some(config) = super::mutant_verify::config(dir, "HEAD")? else {
        return Err(Stop::Usage(format!("mutant sweep: {repo} has no [mutant] run in .safe-push")));
    };
    if config.sources.is_empty() {
        return Err(Stop::Usage(format!("mutant sweep: {repo} lists no [mutant] source pathspec in .safe-push")));
    }
    let evidence = env.state_home.join("agents/test-evidence").join(format!("{repo}.tsv"));
    if !evidence.is_file() {
        return Err(Stop::Fail(format!("mutant sweep: {repo} has no catch evidence at {}; NOT mutating", evidence.display())));
    }
    let store = sweeps(env, &repo);
    fs::create_dir_all(&store)?;
    let matrix = run_matrix(dir, args, &format!("{repo}-{date}"))?;
    fs::write(store.join(format!("{date}.tsv")), matrix)?;
    let mut matrices: Vec<PathBuf> = fs::read_dir(&store)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "tsv"))
        .filter(|path| path.file_stem().and_then(|stem| stem.to_str()).is_some_and(|stem| stem >= since))
        .collect();
    matrices.sort();
    retire(&RetireArgs { repo, matrices, evidence, contracts: config.contracts, date: date.to_owned(), days: args.days })
}

pub(super) fn today() -> String {
    iso(OffsetDateTime::now_utc().date())
}

pub(super) fn sweep(args: &SweepArgs, env: &Env, issues: &dyn Issues, out: &mut dyn Write) -> Result<(), Stop> {
    let date = today();
    let since = since(&date, args.days)?;
    let mut failed = false;
    for dir in &args.repos {
        match sweep_repo(dir, args, env, &date, &since) {
            Ok(line) => {
                writeln!(out, "{line}")?;
                if let Some((repo, number)) = &args.issue {
                    post(issues, repo, *number, &line, out)?;
                }
            }
            Err(Stop::Usage(why) | Stop::Fail(why)) => {
                writeln!(out, "{why}")?;
                failed = true;
            }
            Err(Stop::Refused) => failed = true,
        }
    }
    if failed { Err(Stop::Refused) } else { Ok(()) }
}

#[cfg(test)]
#[path = "mutant_sweep_tests.rs"]
mod tests;
