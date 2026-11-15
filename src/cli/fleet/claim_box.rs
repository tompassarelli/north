//! The box check in front of `north fleet launch` (north#1152): a box text that names no box of its issue is refused
//! before placement or any claim, naming the boxes the issue has, since the ledger would refuse that claim anyway and a
//! refusal that came back late started the worker holding nothing.
use std::collections::BTreeMap;

use crate::dispatch::issue_text;
use crate::error::{NorthError, NorthResult};
use crate::fleet::claims::{claim_keys, split_key};
use crate::fleet::model::LaunchRequest;
use crate::github_mirror::store::{Mirror, OPEN_ISSUES, key};
use crate::threads::catalog::CATALOG;
use crate::threads::claims::{box_slug, boxes, names_box};

/// How many characters of each box a refusal shows.
const SHOWN: usize = 90;

/// An issue's body and creation time as unix seconds, or why GitHub did not give them.
pub(super) type IssueRead = Result<(String, f64), String>;

/// The issues one launch reads from GitHub, each read at most once, so the box check and the tier step share a read.
pub(super) struct IssueReads {
    fetch: Box<dyn FnMut(&str) -> IssueRead>,
    cache: BTreeMap<String, IssueRead>,
}

impl IssueReads {
    pub(super) fn github() -> Self {
        Self::new(Box::new(issue_text))
    }

    fn new(fetch: Box<dyn FnMut(&str) -> IssueRead>) -> Self {
        Self { fetch, cache: BTreeMap::new() }
    }

    pub(super) fn read(&mut self, item: &str) -> IssueRead {
        if !self.cache.contains_key(item) {
            let text = (self.fetch)(item);
            self.cache.insert(item.to_owned(), text);
        }
        self.cache[item].clone()
    }
}

pub(super) fn admit(request: &LaunchRequest, issues: &mut IssueReads) -> NorthResult<()> {
    if request.verification {
        return Ok(());
    }
    let keys = claim_keys(&request.tasks(), request.claimed_box().as_deref(), &request.other_claims());
    refusal(&Mirror::local(), issues, &keys).map_or(Ok(()), |why| Err(NorthError::Refused(why)))
}

/// The refusal of the first box claim among `keys` that names no box of its issue: the mirror's copy of the issue settles a box it holds, and GitHub, through `issues`, one it lacks, since the mirror can lag an edit; an issue neither can read is left to the ledger.
fn refusal(mirror: &Mirror, issues: &mut IssueReads, keys: &[String]) -> Option<String> {
    keys.iter().find_map(|claim| {
        let (task, Some(text)) = split_key(claim) else { return None };
        let (repo, number) = CATALOG.parse_ref(task).ok().filter(|(repo, _)| !CATALOG.is_native(repo))?;
        let Some(slug) = box_slug(text) else { return Some(format!("--box {text} names no word of a box")) };
        if mirrored(mirror, &repo, number).is_some_and(|body| names_box(&body, &slug)) {
            return None;
        }
        let (body, _) = issues.read(task).ok()?;
        let shown: Vec<String> = boxes(&body).map(|text| text.chars().take(SHOWN).collect()).collect();
        (!names_box(&body, &slug)).then(|| format!("--box {text} names no box of {repo}#{number}; give the first words of one of its boxes: {}", shown.join(" | ")))
    })
}

fn mirrored(mirror: &Mirror, repo: &str, number: u64) -> Option<String> {
    let issues = mirror.read(&key(repo, OPEN_ISSUES))?.body;
    let issue = issues.as_array()?.iter().find(|issue| issue["number"].as_u64() == Some(number))?;
    issue["body"].as_str().map(str::to_owned)
}
