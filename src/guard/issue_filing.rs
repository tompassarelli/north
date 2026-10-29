use super::gh_write::{self, GhWrite};
use super::issue_labels::gh;
use crate::hook::{HookInput, Verdict};
use regex::{Captures, Regex};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::LazyLock;

const OWNER: &str = "tompassarelli/";
const SIMILAR: f64 = 0.75;
const FUZZY_WORDS: usize = 3;
const STOP_WORDS: [&str; 14] = ["a", "an", "the", "of", "to", "in", "on", "for", "and", "or", "with", "from", "by", "is"];

static BLOCKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\s*Blocks:\s*((?:[\w.-]+/)?[\w.-]*)#(\d+)").unwrap());
static SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?mi)^[^\w\n]*(?:split|follow[- ]?up)\s+(?:of|to)\b[\s:*_]*((?:[\w.-]+/)?[\w.-]*)#(\d+)").unwrap());
static ISSUE_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https://github\.com/([\w.-]+/[\w.-]+)/issues/(\d+)").unwrap());

pub(super) trait World {
    fn repo(&mut self, named: Option<&str>) -> Option<String>;
    fn claimed(&mut self) -> Vec<String>;
    fn open(&mut self, repo: &str) -> Vec<(u64, String)>;
}

struct Live {
    cwd: Option<String>,
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
        let path = var("THREADS_DB").map(PathBuf::from).or_else(|| {
            var("HOME").map(|home| PathBuf::from(home).join(".local/state/threads/threads.db"))
        });
        let claims = path
            .and_then(|path| crate::threads::claims::ledger::open(&path, false).ok())
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
}

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::hook::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    decide_with(input, &mut Live { cwd: input.cwd().map(str::to_owned) })
}

pub fn link(input: &HookInput) -> Verdict {
    if crate::hook::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    link_with(input, |new, blocked| {
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
    })
}

type Ref = (String, u64);

pub(super) fn link_with(input: &HookInput, mut connect: impl FnMut(&Ref, &Ref) -> Option<()>) -> Verdict {
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
    let writes = gh_write::writes(&text.lossy(), input.cwd());
    let creates: Vec<_> = writes.iter().filter(|write| write.creates_issue).collect();
    if creates.len() != 1 {
        return Verdict::Allow;
    }
    for write in creates {
        let repo = write.repo.clone().unwrap_or_else(|| new.0.clone());
        for capture in BLOCKS.captures_iter(&body(write)) {
            let blocked = (qualified(&capture, &repo), capture[2].parse().unwrap_or(0));
            notes.push(match connect(&new, &blocked) {
                Some(()) => format!("Linked {}#{} as blocking {}#{}.", new.0, new.1, blocked.0, blocked.1),
                None => format!(
                    "Could not link {}#{} as blocking {}#{}; add it with `gh api -X POST repos/{}/issues/{}/dependencies/blocked_by -F issue_id=<id of the new issue>`.",
                    new.0, new.1, blocked.0, blocked.1, blocked.0, blocked.1
                ),
            });
        }
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
    for write in gh_write::writes(&text.lossy(), input.cwd()).iter().filter(|write| write.creates_issue) {
        if let Some(reason) = refusal(write, world) {
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
    format!("{}#{number}", repo.to_lowercase())
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
                return Some(own_task(target, number));
            }
        }
    }
    let title = write.title.as_deref()?;
    let (number, existing) = world.open(&repo).into_iter().find(|(_, open)| similar(title, open))?;
    Some(format!(
        "No. \"{title}\" matches the open issue {repo}#{number} \"{existing}\". Add your boxes or evidence to it with `gh issue comment {number} -R {repo}` or `gh issue edit {number} -R {repo}`; a rolling alarm or standing issue is edited, not filed again."
    ))
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

#[cfg(test)]
#[path = "issue_filing_tests.rs"]
mod tests;
