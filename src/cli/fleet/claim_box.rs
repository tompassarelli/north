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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::fleet::claims::claim_key;

    const BODY: &str = "## Done when\n- [x] Done when: the cause is named with the record that shows it\n- [ ] Done when: with that cause present, a launch starts its worker holding its claim within 5 s\n";

    fn counting(reads: &Rc<Cell<usize>>) -> IssueReads {
        let reads = Rc::clone(reads);
        IssueReads::new(Box::new(move |_: &str| -> IssueRead {
            reads.set(reads.get() + 1);
            Ok((BODY.to_owned(), 0.0))
        }))
    }

    /// On 2026-11-11 two launches whose `--box` was a phrase from inside a box's Done-when command started their workers holding no claim, and a third was refused after 30 s with a message that blamed the ledger.
    #[test]
    fn a_box_text_that_names_no_box_is_refused_before_any_claim_naming_the_issues_boxes() {
        let directory = tempfile::tempdir().unwrap();
        let mirror = Mirror::at(directory.path().join("mirror.db"));
        mirror.write(&key("north", OPEN_ISSUES), &serde_json::json!([{"number": 1152, "body": BODY}]), None, 0.0).unwrap();
        let reads = Rc::new(Cell::new(0));
        let mut issues = counting(&reads);

        let held = [claim_key("north#1152", Some("Done when: with that cause present"))];
        assert_eq!(refusal(&mirror, &mut issues, &held), None);

        let wrong = [claim_key("north#1152", Some("an_ordered_promotion_of_a_landed_commit"))];
        let why = refusal(&mirror, &mut issues, &wrong).expect("a box text that names no box is refused");
        assert!(why.contains("names no box of north#1152") && why.contains("Done when: the cause is named") && why.contains("Done when: with that cause present"), "{why}");
        assert_eq!(reads.get(), 1, "a box the mirror holds is settled without GitHub, and one it lacks is read from GitHub before the launch refuses it");
    }

    #[test]
    fn a_launch_reads_the_issue_body_once_for_its_box_check_and_its_tier_step() {
        let directory = tempfile::tempdir().unwrap();
        let mirror = Mirror::at(directory.path().join("mirror.db"));
        let reads = Rc::new(Cell::new(0));
        let mut issues = counting(&reads);

        let wrong = [claim_key("north#1152", Some("an_ordered_promotion_of_a_landed_commit"))];
        assert!(refusal(&mirror, &mut issues, &wrong).is_some());
        let (body, _) = issues.read("north#1152").expect("the tier step reads the issue the box check read");
        assert!(body.contains("with that cause present"));
        assert_eq!(reads.get(), 1);
    }
}
