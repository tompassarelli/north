use super::gh_write::{self, GhWrite};
use super::issue_labels::gh;
use crate::hook_input::{HookInput, Verdict};
use regex::{Captures, Regex};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::LazyLock;

const OWNER: &str = "tompassarelli/";
const SIMILAR: f64 = 0.75;
const FUZZY_WORDS: usize = 3;
const STOP_WORDS: [&str; 14] = ["a", "an", "the", "of", "to", "in", "on", "for", "and", "or", "with", "from", "by", "is"];

static BLOCKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^[^\w\n]*Blocks:[ \t*_]*((?:[\w.-]+/)?[\w.-]*)#(\d+)").unwrap());
static SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?mi)^[^\w\n]*(?:split|follow[- ]?up)\s+(?:of|to)\b[\s:*_]*((?:[\w.-]+/)?[\w.-]*)#(\d+)").unwrap());
static ISSUE_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https://github\.com/([\w.-]+/[\w.-]+)/issues/(\d+)").unwrap());
static FILE_IT_FIRST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^[^\w\n]*Item:[^\n]*\bfile it first\b").unwrap());
static DEFECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^[^\w\n]*Defect:[\s*_]*\S").unwrap());
static DONE_WHEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^[^\w\n]*Done when:[ \t*_]*").unwrap());
static BOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[ \t>]*[-*][ \t]+\[[ xX]\]").unwrap());
static CATEGORY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^[ \t>*-]*\**Category\**:[ \t*_`]*([A-Za-z][\w-]*)").unwrap());
static CLASS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^[ \t>*-]*\**Class\**:[ \t*_`]*\S").unwrap());
static FIELD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z][\w -]*:").unwrap());
static MARKER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[ \t>*-]*(?:\[[ xX]\][ \t]*)?").unwrap());
static PARENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^[^\w\n]*Parent:[ \t*_]*((?:[\w.-]+/)?[\w.-]*)#(\d+)").unwrap());
static RUNNABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^[^\w\n]*Done when:[ \t*_]*[^\n]*`[^`\n]+`").unwrap());
const CHILD_LIMIT: &str = "worker_children_per_session";
const BRIEF_LINES: usize = 64;
const BUG_CATEGORIES: [&str; 2] = ["bug-known-cause", "debugging-unknown-cause"];
const CATEGORY_LABELS: [(&str, &str); 3] = [("enhancement", "feature"), ("bug", "debugging-unknown-cause"), ("documentation", "docs-policy")];
const ROOT_LABEL: &str = "roadmap";

pub(super) trait World {
    fn repo(&mut self, named: Option<&str>) -> Option<String>;
    fn claimed(&mut self) -> Vec<String>;
    fn open(&mut self, repo: &str) -> Vec<(u64, String)>;
    fn worker_brief(&mut self) -> Option<String>;
    fn children_filed(&mut self) -> usize;
}

struct Live {
    cwd: Option<String>,
    transcript: Option<PathBuf>,
    fleet_session: Option<String>,
}

impl World for Live {
    fn repo(&mut self, named: Option<&str>) -> Option<String> {
        match named {
            Some(repo) => Some(repo.to_owned()),
            None => gh(self.cwd.as_deref(), &["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"])
                .map(|out| out.trim().to_owned()),
        }
    }

    fn claimed(&mut self) -> Vec<String> {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        let Some(actor) = var("THREADS_ACTOR").or_else(|| var("USER")) else {
            return Vec::new();
        };
        let claims = crate::threads::claims::ledger::open(&crate::threads::client::threads_database(), false)
            .ok()
            .and_then(|db| crate::threads::claims::ledger::claims(&db).ok());
        claims
            .into_iter()
            .flatten()
            .filter(|claim| claim.owner == actor)
            .map(|claim| claim.item)
            .collect()
    }

    fn open(&mut self, repo: &str) -> Vec<(u64, String)> {
        let listed = gh(
            None,
            &["issue", "list", "-R", repo, "--state", "open", "--json", "number,title", "-L", "500", "-q", r#".[] | "\(.number)\t\(.title)""#],
        );
        listed
            .iter()
            .flat_map(|out| out.lines())
            .filter_map(|line| {
                let (number, title) = line.split_once('\t')?;
                Some((number.parse().ok()?, title.to_owned()))
            })
            .collect()
    }

    fn children_filed(&mut self) -> usize {
        let Some(file) = self.transcript.as_ref().and_then(|path| std::fs::File::open(path).ok()) else {
            return 0;
        };
        std::io::BufRead::lines(std::io::BufReader::new(file))
            .map_while(Result::ok)
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
            .filter(|entry| entry["type"] == "assistant")
            .flat_map(|entry| entry["message"]["content"].as_array().cloned().unwrap_or_default())
            .filter_map(|part| part["input"]["command"].as_str().map(str::to_owned))
            .flat_map(|command| gh_write::writes(&command, None))
            .filter(|write| write.creates_issue && PARENT.is_match(&body(write)))
            .count()
    }

    fn worker_brief(&mut self) -> Option<String> {
        self.fleet_session.as_ref()?;
        let file = std::fs::File::open(self.transcript.as_ref()?).ok()?;
        let prompt = std::io::BufRead::lines(std::io::BufReader::new(file))
            .take(BRIEF_LINES)
            .map_while(Result::ok)
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
            .find(|entry| entry["type"] == "user")
            .and_then(|entry| match &entry["message"]["content"] {
                serde_json::Value::String(text) => Some(text.clone()),
                serde_json::Value::Array(parts) => Some(parts.iter().filter_map(|part| part["text"].as_str()).collect::<Vec<_>>().join("\n")),
                _ => None,
            })?;
        DONE_WHEN.is_match(&prompt).then_some(prompt)
    }
}

pub fn evaluate(input: &HookInput) -> Verdict {
    decide_with(
        input,
        &mut Live {
            cwd: input.cwd().map(str::to_owned),
            transcript: input.data["transcript_path"].as_str().map(PathBuf::from),
            fleet_session: std::env::var("NORTH_FLEET_SESSION").ok().filter(|session| !session.is_empty()),
        },
    )
}

pub fn link(input: &HookInput) -> Verdict {
    link_with(
        input,
        &mut Live {
            cwd: input.cwd().map(str::to_owned),
            transcript: input.data["transcript_path"].as_str().map(PathBuf::from),
            fleet_session: std::env::var("NORTH_FLEET_SESSION").ok().filter(|session| !session.is_empty()),
        },
    )
}

pub(super) type Ref = (String, u64);

/// What the hook does after an issue is filed: link what it blocks and claim it for the fleet worker that filed it unless it blocks another issue.
pub(super) trait Filed {
    fn connect(&mut self, new: &Ref, blocked: &Ref) -> Option<()>;
    /// The fleet worker session filing, never the proxy, which files issues for dispatch to staff.
    fn worker(&mut self) -> Option<String>;
    fn claim(&mut self, new: &Ref, by: &str) -> Result<String, String>;
    /// Whether the worker's brief makes the issue it files its own item (`Item: new ... issue, file it first`), so one
    /// naming what it blocks stays with it too.
    fn files_its_item(&mut self) -> bool;
}

impl Filed for Live {
    fn connect(&mut self, new: &Ref, blocked: &Ref) -> Option<()> {
        let id = gh(None, &["api", &format!("repos/{}/issues/{}", new.0, new.1), "--jq", ".id"])?;
        gh(
            None,
            &[
                "api",
                "-X",
                "POST",
                &format!("repos/{}/issues/{}/dependencies/blocked_by", blocked.0, blocked.1),
                "-F",
                &format!("issue_id={}", id.trim()),
            ],
        )
        .map(drop)
    }

    fn worker(&mut self) -> Option<String> {
        let role = std::env::var(crate::fleet::runtime::ROLE_ENV).unwrap_or_default();
        self.fleet_session.clone().filter(|_| role != crate::fleet::runtime::PROXY)
    }

    fn claim(&mut self, new: &Ref, by: &str) -> Result<String, String> {
        let north = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("north"));
        let ran = std::process::Command::new(north)
            .args(["fleet", "hold", &format!("{}#{}", new.0, new.1)])
            .env(crate::fleet::runtime::SESSION_ENV, by)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| error.to_string())?;
        let said = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_owned();
        if ran.status.success() { Ok(said(&ran.stdout)) } else { Err(said(&ran.stderr)) }
    }

    fn files_its_item(&mut self) -> bool {
        self.worker_brief().is_some_and(|brief| FILE_IT_FIRST.is_match(&brief))
    }
}

pub(super) fn link_with(input: &HookInput, world: &mut impl Filed) -> Verdict {
    if !input.is_shell() || input.data["hook_event_name"] != "PostToolUse" {
        return Verdict::Allow;
    }
    let Some(text) = input.command() else {
        return Verdict::Allow;
    };
    let raw = String::from_utf8_lossy(&input.raw).replace("\\/", "/");
    let Some(response) = raw.split_once("\"tool_response\"").map(|(_, response)| response) else {
        return Verdict::Allow;
    };
    let Some(url) = ISSUE_URL.captures(response) else {
        return Verdict::Allow;
    };
    let new: Ref = (url[1].to_owned(), url[2].parse().unwrap_or(0));
    let mut notes = Vec::new();
    let writes = gh_write::writes(text, input.cwd());
    let [write] = writes.iter().filter(|write| write.creates_issue).collect::<Vec<_>>()[..] else {
        return Verdict::Allow;
    };
    let repo = write.repo.clone().unwrap_or_else(|| new.0.clone());
    let body = body(write);
    for capture in BLOCKS.captures_iter(&body) {
        let blocked = (qualified(&capture, &repo), capture[2].parse().unwrap_or(0));
        notes.push(match world.connect(&new, &blocked) {
            Some(()) => format!("Linked {}#{} as blocking {}#{}.", new.0, new.1, blocked.0, blocked.1),
            None => format!(
                "Could not link {}#{} as blocking {}#{}; add it with `gh api -X POST repos/{}/issues/{}/dependencies/blocked_by -F issue_id=<id of the new issue>`.",
                new.0, new.1, blocked.0, blocked.1, blocked.0, blocked.1
            ),
        });
    }
    if let Some(worker) = world.worker().filter(|_| !BLOCKS.is_match(&body) || world.files_its_item()) {
        notes.push(match world.claim(&new, &worker) {
            Ok(said) => format!("{said}: an issue a fleet worker files is claimed for it, so north fleet dispatch does not staff it again."),
            Err(why) => format!("Could not claim {}#{} for you ({why}); hold it with `north fleet hold {}#{}` before north fleet dispatch staffs it again.", new.0, new.1, new.0, new.1),
        });
    }
    if notes.is_empty() { Verdict::Allow } else { Verdict::Advise(notes.join(" ")) }
}

pub(super) fn decide_with(input: &HookInput, world: &mut impl World) -> Verdict {
    if !input.is_shell() || !input.raw.windows(2).any(|bytes| bytes == b"gh") {
        return Verdict::Allow;
    }
    let Some(text) = input.command() else {
        return Verdict::Allow;
    };
    for mut write in gh_write::writes(text, input.cwd()).into_iter().filter(|write| write.creates_issue) {
        if write.opaque || write.unread {
            write.bodies.push(text.replace("\\n", "\n"));
        }
        if let Some(reason) = refusal(&write, world) {
            return Verdict::Deny(reason);
        }
    }
    Verdict::Allow
}

fn body(write: &GhWrite) -> String {
    let mut skipped = false;
    let lines: Vec<&str> = write
        .bodies
        .iter()
        .filter(|text| {
            let is_title = !skipped && write.title.as_deref() == Some(text.as_str());
            skipped |= is_title;
            !is_title
        })
        .map(String::as_str)
        .collect();
    lines.join("\n")
}

fn qualified(capture: &Captures, repo: &str) -> String {
    match capture[1].trim_end_matches('/') {
        "" => repo.to_owned(),
        named if named.contains('/') => named.to_owned(),
        named => format!("{}{named}", OWNER),
    }
}

fn key(repo: &str, number: &str) -> String {
    let lowered = repo.to_lowercase();
    match lowered.strip_prefix(OWNER) {
        Some(name) => format!("{OWNER}{}#{number}", crate::threads::catalog::CATALOG.canonical(name)),
        None => format!("{lowered}#{number}"),
    }
}

fn claim_key(item: &str) -> String {
    match item.split_once('#') {
        Some((name, number)) if !name.contains('/') => key(&format!("{OWNER}{name}"), number),
        _ => item.to_lowercase(),
    }
}

fn refusal(write: &GhWrite, world: &mut impl World) -> Option<String> {
    let repo = world.repo(write.repo.as_deref())?;
    if !repo.to_lowercase().starts_with(OWNER) {
        return None;
    }
    let body = body(write);
    let mut reasons = Vec::new();
    let splits: Vec<_> = SPLIT.captures_iter(&body).map(|capture| (qualified(&capture, &repo), capture[2].to_owned())).collect();
    if !splits.is_empty() {
        let claimed: Vec<String> = world.claimed().iter().map(|item| claim_key(item)).collect();
        let blocking: Vec<String> = BLOCKS
            .captures_iter(&body)
            .map(|capture| key(&qualified(&capture, &repo), &capture[2]))
            .collect();
        for (target, number) in &splits {
            let id = key(target, number);
            if claimed.contains(&id) && !blocking.contains(&id) {
                reasons.push(own_task(target, number));
            }
        }
    }
    if let Some(parent) = PARENT.captures(&body) {
        reasons.extend(child_refusals(write, &body, &(qualified(&parent, &repo), parent[2].to_owned()), &repo, world));
        return (!reasons.is_empty()).then(|| reasons.join("\n"));
    }
    reasons.extend(shape(write, &body));
    if let Some(brief) = world.worker_brief()
        && !BLOCKS.is_match(&body)
        && !DEFECT.is_match(&body)
        && !records_brief(&brief, &body)
    {
        reasons.push(if write.opaque { format!("{UNFOUNDED}{OPAQUE}") } else { UNFOUNDED.to_owned() });
    }
    if let Some(title) = write.title.as_deref()
        && let Some((number, existing)) = world.open(&repo).into_iter().find(|(_, open)| similar(title, open))
    {
        reasons.push(format!(
            "No. \"{title}\" matches the open issue {repo}#{number} \"{existing}\". Add your boxes or evidence to it with `gh issue comment {number} -R {repo}` or `gh issue edit {number} -R {repo}`; a rolling alarm or standing issue is edited, not filed again."
        ));
    }
    (!reasons.is_empty()).then(|| reasons.join("\n"))
}

fn child_refusals(write: &GhWrite, body: &str, parent: &(String, String), repo: &str, world: &mut impl World) -> Vec<String> {
    let mut reasons = Vec::new();
    if !parent.0.eq_ignore_ascii_case(repo) {
        reasons.push(format!("No. The parent {}#{} is in another repository than the child; a child lives beside its parent.", parent.0, parent.1));
    }
    if !RUNNABLE.is_match(body) {
        reasons.push("No. A child issue needs a runnable `Done when: `COMMAND`` line: the command that closes it when it passes.".to_owned());
    }
    reasons.extend(shape(write, body).into_iter().filter(|reason| reason != NO_BOX));
    if world.worker_brief().is_some() {
        let held: Vec<String> = world.claimed().iter().map(|item| claim_key(item)).collect();
        if !held.contains(&key(&parent.0, &parent.1)) {
            reasons.push(format!("No. A worker files a child only of the issue it holds, and you do not hold {}#{}. An issue with no parent is the owner's to authorize.", parent.0, parent.1));
        }
        let limit = north_base::platform::harness::default_number(CHILD_LIMIT) as usize;
        if world.children_filed() >= limit {
            reasons.push(format!("No. A session files at most {limit} child issues; put the rest under `Ideas:` in your final report."));
        }
    }
    reasons
}

const NO_BOX: &str = "No. The body has no `- [ ]` box, and north fleet dispatch staffs only issues with an unticked box. Write each check as a `- [ ]` line under a `## Done when` heading, one per guarantee, each naming the command or observation that proves it.";
const UNFOUNDED: &str = "No. A worker files an issue only for what it cannot finish in its lane: a blocker, with a `Blocks: repo#N` body line naming the issue it blocks, or a measured defect, with a `Defect:` body line giving the command you ran and what it showed. A finding you can fix in 20 minutes is fixed in this lane and lands with it. An idea or a possible follow-up goes under `Ideas:` in your final report, not into a new issue. The issue for the item you were launched on passes when its body carries your brief's Done when.";
const OPAQUE: &str = " This body is built from a shell variable or a pipe the guard cannot read, so give it literally, in a heredoc or with --body-file PATH.";

fn spaced(text: &str) -> String {
    MARKER.replace_all(text, "").split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn records_brief(brief: &str, body: &str) -> bool {
    let body = spaced(body);
    DONE_WHEN.find_iter(brief).any(|found| {
        let rest = &brief[found.end()..];
        let (first, later) = rest.split_once('\n').unwrap_or((rest, ""));
        let list = later.lines().take_while(|line| !line.trim().is_empty() && !FIELD.is_match(line));
        let outcome: String = spaced(&std::iter::once(first).chain(list).collect::<Vec<_>>().join("\n")).chars().take(80).collect();
        outcome.len() >= 20 && body.contains(&outcome)
    })
}

fn own_task(repo: &str, number: &str) -> String {
    format!(
        "No. This body is a split or follow-up of {repo}#{number}, the issue you claimed; the pieces of your own task are boxes on that issue, not new issues. Add them with `gh issue view {number} -R {repo} --json body -q .body > /tmp/{number}.md`, append each piece as a `- [ ] (verification class) outcome` line under Done when, then `gh issue edit {number} -R {repo} --body-file /tmp/{number}.md`. File a new issue only for a blocker (a `Blocks: {repo}#N` body line, which links it) or an independent outcome."
    )
}

fn words(title: &str) -> BTreeSet<String> {
    title
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty() && !STOP_WORDS.contains(word))
        .map(str::to_owned)
        .collect()
}

fn similar(title: &str, other: &str) -> bool {
    let (left, right) = (words(title), words(other));
    if left.is_empty() || right.is_empty() {
        return false;
    }
    if left == right {
        return true;
    }
    if left.len().min(right.len()) < FUZZY_WORDS {
        return false;
    }
    let shared = left.intersection(&right).count() as f64;
    shared / left.union(&right).count() as f64 >= SIMILAR
}


const NO_CATEGORY: &str = "No. The body has no `Category:` line and no label names one (`category:NAME`, enhancement, bug or documentation), and north fleet dispatch places no box without a category. Add a `Category:` line before the first blank line, such as `Category: tooling`.";
const NO_CLASS: &str = "No. A bug category (bug-known-cause, debugging-unknown-cause, or the bug label) needs a `Class:` line, and north fleet dispatch places no such box without one. Add `Class:` before the first blank line naming the class of failure, and write a Done when that covers every instance of it.";

/// The category dispatch would place the issue under: its `Category:` line, else a category label.
fn category(body: &str, labels: &[String]) -> Option<String> {
    let named = CATEGORY.captures(body).map(|found| found[1].to_lowercase());
    let labelled = || {
        labels.iter().find_map(|label| label.strip_prefix("category:").filter(|name| !name.is_empty()).map(str::to_owned)).or_else(|| CATEGORY_LABELS.iter().find(|(label, _)| labels.iter().any(|has| has == label)).map(|(_, category)| (*category).to_owned()))
    };
    named.or_else(labelled)
}

/// Why dispatch could not staff the new issue: no box, no category, or a bug category with no `Class:` line. A roadmap root is finished by its children, so it needs none.
fn shape(write: &GhWrite, body: &str) -> Vec<String> {
    if write.labels.iter().any(|label| label == ROOT_LABEL) {
        return Vec::new();
    }
    let mut reasons = Vec::new();
    if !BOX.is_match(body) {
        reasons.push(NO_BOX.to_owned());
    }
    match category(body, &write.labels) {
        None => reasons.push(NO_CATEGORY.to_owned()),
        Some(category) if let Err(why) = crate::router::brief::known_category(&category) => reasons.push(format!("No. {why}, and north fleet dispatch places no box under another word.")),
        Some(category) if BUG_CATEGORIES.contains(&category.as_str()) && !CLASS.is_match(body) => reasons.push(NO_CLASS.to_owned()),
        Some(_) => {}
    }
    reasons
}

#[cfg(test)]
#[path = "issue_filing_tests.rs"]
mod tests;
