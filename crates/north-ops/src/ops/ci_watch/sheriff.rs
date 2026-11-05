use std::io::Write;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use super::{Call, TITLE, gh, gh_json};

const USAGE: &str = "usage: north main-red RUN_ID";
const BODY_LIMIT: usize = 60_000;
const SECTION: &str = "\n\n<!-- main-red:";
const DONE_WHEN: &str = "Main CI failures and confirmed culprit decisions.\n\n## Done when\n- [ ] The newest Harness and Cargo runs on main are green on a commit after the newest culprit range below: `gh run list --branch main --workflow Harness --limit 1` and the same for Cargo both print success; close this issue with that run's link.";

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Step {
    pub name: String,
    pub conclusion: Option<String>,
    pub number: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Job {
    pub id: u64,
    pub name: String,
    pub conclusion: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Commit {
    pub sha: String,
    pub parents: Vec<String>,
    pub message: String,
    pub files: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Previous {
    pub sha: String,
    pub conclusion: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Successor {
    pub status: String,
    pub conclusion: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Situation {
    pub attempt: u64,
    pub conclusion: Option<String>,
    pub original: Vec<Job>,
    pub confirmed: Vec<Job>,
    pub previous: Option<Previous>,
    pub commits: Vec<Commit>,
    pub on_main: bool,
    pub head: String,
    pub tip: String,
    pub successor: Option<Successor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    None,
    Report,
    Confirm,
    Request,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub kind: Kind,
    pub reason: String,
    pub step: Option<String>,
    pub job: Option<u64>,
    pub shas: Vec<String>,
    pub parent: Option<String>,
}

impl Decision {
    fn new(kind: Kind, reason: impl Into<String>) -> Self {
        Self { kind, reason: reason.into(), step: None, job: None, shas: Vec::new(), parent: None }
    }
}

static SETUP_STEP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(Set up job|Complete job|Post |Install |Checkout |Cache |Activate |Read setup |Toolchain |Run actions/|Run oven-sh/)").expect("setup pattern")
});
static REVERT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^Revert\b|^Sheriff-Reverts:").expect("revert pattern"));
static FAILURE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"---- .* stdout ----|test .* \.\.\. FAILED|land check: \S+\.(sh|py) (exited|was killed)|\bFAIL:? ").expect("failure pattern")
});

fn failed(conclusion: &Option<String>) -> bool {
    conclusion.as_deref() == Some("failure")
}

pub fn decide(s: &Situation) -> Decision {
    if matches!(s.conclusion.as_deref(), None | Some("cancelled")) {
        return Decision::new(Kind::None, "The run was cancelled or never concluded; nothing to revert");
    }
    let mut original: Vec<&Job> = s.original.iter().collect();
    original.sort_by_key(|j| j.id);
    let job = original.into_iter().find(|j| failed(&j.conclusion) && j.steps.iter().any(|t| failed(&t.conclusion)));
    let Some((job, step)) = job.and_then(|job| job.steps.iter().find(|t| failed(&t.conclusion)).map(|step| (job, step))) else {
        return Decision::new(if failed(&s.conclusion) { Kind::Report } else { Kind::None }, "No failing test step was recorded");
    };
    let name = format!("{}: {}", job.name, step.name);
    let report = |reason: &str| Decision { step: Some(name.clone()), ..Decision::new(Kind::Report, reason) };
    if !s.on_main {
        return report("The failing commit is no longer on main; report only");
    }
    if s.tip != s.head {
        let tip = &s.tip;
        return report(&match &s.successor {
            Some(next) if next.status != "completed" => {
                format!("Main moved to {tip}, whose run is still {}: a fix-forward may land, so revert nothing", next.status)
            }
            Some(next) if next.conclusion.as_deref() == Some("success") => {
                format!("Main moved to {tip}, whose run passed: a fix-forward landed, so revert nothing")
            }
            _ => format!("Main moved past the failing commit to {tip}; a later landing may already fix it, so never revert on this stale run"),
        });
    }
    if s.attempt == 1 {
        return Decision {
            step: Some(name),
            job: Some(job.id),
            ..Decision::new(Kind::Confirm, "Confirm this failed job once; retain the original failure")
        };
    }
    if s.attempt != 2 {
        return report("Confirmation budget exhausted; never rerun again");
    }
    let confirmed = s.confirmed.iter().find(|j| j.name == job.name);
    let repeated = confirmed.is_some_and(|c| failed(&c.conclusion) && c.steps.iter().any(|t| t.name == step.name && failed(&t.conclusion)));
    if !failed(&s.conclusion) || !repeated {
        return report("The original step did not fail again; no confirmed culprit");
    }
    if step.name.ends_with("planted fault") {
        return report("A canary failed: a check stopped checking or its planted fault no longer applies; report, never revert");
    }
    if SETUP_STEP.is_match(&step.name) {
        return report("A setup step failed; report infrastructure failure");
    }
    let Some(previous) = s.previous.as_ref().filter(|p| p.conclusion.as_deref() == Some("success")) else {
        return report("No earlier green main run of this workflow is an ancestor of the failing commit");
    };
    if s.commits.is_empty() {
        return report("No landed commits after the previous run");
    }
    let mut parent = previous.sha.as_str();
    for commit in s.commits.iter().rev() {
        if commit.parents.len() != 1 || commit.parents[0] != parent {
            return report("The green run is not the landing's parent");
        }
        if REVERT.is_match(&commit.message) {
            return report("Never revert a revert");
        }
        if commit.files.iter().any(|p| p.starts_with(".github/workflows/")) {
            return report("Never revert workflow changes");
        }
        parent = &commit.sha;
    }
    if parent != s.head {
        return report("The landed range does not end at the failing commit");
    }
    Decision {
        step: Some(name),
        job: Some(job.id),
        shas: s.commits.iter().map(|c| c.sha.clone()).collect(),
        parent: Some(previous.sha.clone()),
        ..Decision::new(
            Kind::Request,
            format!(
                "Same step failed twice after a green parent; the lander reverts the one of {} landings whose removal turns the failing cases green",
                s.commits.len()
            ),
        )
    }
}

pub fn revert_request(head: &str, verdict: &Decision, failed: &[&str], url: &str) -> String {
    format!(
        "Main red at {head}\n\n{}\n{}\n\nGreen-parent: {}\nMain-red-run: {url}",
        verdict.step.as_deref().unwrap_or_default(),
        failed.join("\n"),
        verdict.parent.as_deref().unwrap_or_default()
    )
}

pub fn issue_body(existing: Option<&str>, marker: &str, body: &str) -> String {
    let existing = existing.unwrap_or(DONE_WHEN);
    let (head, older) = existing.split_once(SECTION).map_or((existing, ""), |(head, rest)| (head, rest));
    let newest: String = format!("{marker}\n{body}").chars().take(BODY_LIMIT / 2).collect();
    let mut kept: Vec<&str> = Vec::new();
    let mut length = head.len() + newest.len() + 2;
    for section in older.rsplit(SECTION).filter(|section| !section.is_empty()) {
        length += section.len() + SECTION.len();
        if length > BODY_LIMIT {
            break;
        }
        kept.push(section);
    }
    let older: String = kept.iter().rev().map(|section| format!("{SECTION}{section}")).collect();
    format!("{head}{older}\n\n{newest}")
}

#[derive(Debug, Deserialize)]
struct CiRun {
    id: u64,
    workflow_id: u64,
    head_sha: String,
    head_branch: Option<String>,
    event: String,
    run_attempt: u64,
    conclusion: Option<String>,
    html_url: String,
}

#[derive(Debug, Deserialize)]
struct Runs {
    workflow_runs: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct Issue {
    number: u64,
    title: String,
    body: String,
}

struct Repo(String);

impl Repo {
    fn from_env() -> Result<Self, String> {
        let value = std::env::var("GH_REPO").unwrap_or_default();
        let valid = Regex::new(r"^[\w.-]+/[\w.-]+$").expect("repo pattern");
        if !valid.is_match(&value) {
            return Err("GH_REPO must name owner/repository".into());
        }
        Ok(Self(format!("repos/{value}")))
    }

    fn api<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        gh_json(&["api", &format!("{}/{path}", self.0)])
    }

    fn jobs(&self, id: u64, attempt: u64) -> Result<Vec<Job>, String> {
        let path = format!("{}/actions/runs/{id}/attempts/{attempt}/jobs?per_page=100", self.0);
        let pages: Vec<Value> = gh_json(&["api", "--paginate", "--slurp", &path])?;
        let jobs = pages.into_iter().flat_map(|page| page["jobs"].as_array().cloned().unwrap_or_default());
        jobs.map(|job| serde_json::from_value(job).map_err(|error| format!("jobs of run {id}: {error}"))).collect()
    }
}

fn git(args: &[&str]) -> Result<String, String> {
    Call::new("git", args).output().map(|out| out.trim().to_owned())
}

fn is_ancestor(ancestor: &str, descendant: &str) -> bool {
    std::process::Command::new("git").args(["merge-base", "--is-ancestor", ancestor, descendant]).status().is_ok_and(|s| s.success())
}

fn green_parent(history: &[CiRun], id: u64, reaches_head: impl Fn(&str) -> bool) -> Option<&CiRun> {
    history.iter().filter(|r| r.id < id && r.conclusion.as_deref() == Some("success") && reaches_head(&r.head_sha)).max_by_key(|r| r.id)
}

fn open_issue() -> Result<Option<Issue>, String> {
    let search = format!("\"{TITLE}\" in:title");
    let issues: Vec<Issue> = gh_json(&["issue", "list", "--state", "open", "--search", &search, "--json", "number,title,body", "--limit", "100"])?;
    Ok(issues.into_iter().find(|i| i.title == TITLE))
}

fn report(existing: Option<&Issue>, marker: &str, body: &str) -> Result<(), String> {
    let text = issue_body(existing.map(|i| i.body.as_str()), marker, body);
    let number = existing.map(|i| i.number.to_string());
    let mut args = match &number {
        Some(number) => vec!["issue", "edit", number],
        None => vec!["issue", "create", "--title", TITLE],
    };
    args.extend(["--body-file", "-"]);
    Call { input: Some(&text), ..Call::new("gh", &args) }.output()?;
    if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        let mut file = std::fs::OpenOptions::new().append(true).create(true).open(summary).map_err(|error| error.to_string())?;
        writeln!(file, "{body}").map_err(|error| error.to_string())?;
    }
    println!("{body}");
    Ok(())
}

fn landed(prior: &str, head: &str) -> Result<Vec<Commit>, String> {
    let log = git(&["log", "--first-parent", "-z", "--format=%H%n%P%n%B", &format!("{prior}..{head}")])?;
    log.split('\0')
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| {
            let mut lines = entry.trim_start().split('\n');
            let sha = lines.next().unwrap_or_default().to_owned();
            let parents = lines.next().unwrap_or_default().split(' ').filter(|p| !p.is_empty()).map(str::to_owned).collect();
            let message = lines.collect::<Vec<_>>().join("\n");
            let files = git(&["diff-tree", "--root", "--no-commit-id", "--name-only", "-r", &sha])?.split('\n').map(str::to_owned).collect();
            Ok(Commit { sha, parents, message, files })
        })
        .collect()
}

pub fn sheriff(id: u64) -> Result<(), String> {
    let repo = Repo::from_env()?;
    let ci: CiRun = repo.api(&format!("actions/runs/{id}"))?;
    if ci.head_branch.as_deref() != Some("main") || ci.event != "push" {
        return Ok(());
    }
    let original = repo.jobs(id, 1)?;
    if ci.conclusion.as_deref() == Some("success") && ci.run_attempt == 1 {
        return Ok(());
    }
    let existing = open_issue()?;
    let marker = format!("<!-- main-red:{id}:{} -->", ci.run_attempt);
    if existing.as_ref().is_some_and(|i| i.body.contains(&marker)) {
        return Ok(());
    }
    git(&["fetch", "-q", "origin", "main"])?;
    let tip = git(&["rev-parse", "origin/main"])?;
    let on_main = is_ancestor(&ci.head_sha, &tip);
    let successor: Runs = repo.api(&format!("actions/workflows/{}/runs?branch=main&event=push&head_sha={tip}&per_page=1", ci.workflow_id))?;
    let successor = successor.workflow_runs.first().map(|run| Successor {
        status: run["status"].as_str().unwrap_or_default().to_owned(),
        conclusion: run["conclusion"].as_str().map(str::to_owned),
    });
    let history: Runs = repo.api(&format!("actions/workflows/{}/runs?branch=main&event=push&status=completed&per_page=100", ci.workflow_id))?;
    let history: Vec<CiRun> = history.workflow_runs.into_iter().filter_map(|run| serde_json::from_value(run).ok()).collect();
    let prior = green_parent(&history, id, |sha| is_ancestor(sha, &ci.head_sha));
    let commits = match prior {
        Some(prior) if is_ancestor(&prior.head_sha, &ci.head_sha) => landed(&prior.head_sha, &ci.head_sha)?,
        _ => Vec::new(),
    };
    let confirmed = if ci.run_attempt == 2 { repo.jobs(id, 2)? } else { Vec::new() };
    let verdict = decide(&Situation {
        attempt: ci.run_attempt,
        conclusion: ci.conclusion.clone(),
        original,
        confirmed,
        previous: prior.map(|p| Previous { sha: p.head_sha.clone(), conclusion: p.conclusion.clone() }),
        commits: commits.clone(),
        on_main,
        head: ci.head_sha.clone(),
        tip,
        successor,
    });
    let range = if commits.is_empty() { ci.head_sha.clone() } else { commits.iter().map(|c| c.sha.as_str()).collect::<Vec<_>>().join(", ") };
    let detail = format!(
        "Culprit range: {range}\nFailing step: {}\n[CI run]({})\n{}",
        verdict.step.as_deref().unwrap_or("unknown"),
        ci.html_url,
        verdict.reason
    );
    match verdict.kind {
        Kind::Confirm => {
            let job = verdict.job.unwrap_or_default();
            report(
                existing.as_ref(),
                &marker,
                &format!("{detail}\nRequested one confirmation of job {job}; a green confirmation is reported, never treated as a repaired run."),
            )?;
            gh(&["api", "--method", "POST", &format!("{}/actions/jobs/{job}/rerun", repo.0)])?;
        }
        Kind::Request => {
            let logs = gh(&["api", &format!("{}/actions/jobs/{}/logs", repo.0, verdict.job.unwrap_or_default())])?;
            let failed: Vec<&str> = logs.split('\n').filter(|line| FAILURE.is_match(line)).take(200).collect();
            let message = revert_request(&ci.head_sha, &verdict, &failed, &ci.html_url);
            let request = git(&["commit-tree", &format!("{}^{{tree}}", ci.head_sha), "-p", &ci.head_sha, "-m", &message])?;
            git(&["push", "origin", &format!("{request}:refs/heads/claude/land-revert-{}", ci.head_sha)])?;
            report(
                existing.as_ref(),
                &marker,
                &format!("{detail}\nAsked the lander to revert the culprit through the landing queue (claude/land-revert-{}).", ci.head_sha),
            )?;
        }
        Kind::Report => report(existing.as_ref(), &marker, &detail)?,
        Kind::None => {}
    }
    Ok(())
}

pub fn run(arguments: &[String]) -> u8 {
    let [id] = arguments else {
        eprintln!("{USAGE}");
        return 2;
    };
    let Some(id) = id.parse::<u64>().ok().filter(|id| *id > 0) else {
        eprintln!("{USAGE}");
        return 2;
    };
    match sheriff(id) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("main-red: {error}");
            1
        }
    }
}

#[cfg(test)]
#[path = "sheriff_tests.rs"]
mod tests;
