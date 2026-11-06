use north_github_mirror::store::{CLOSED_ISSUES, OPEN_ISSUES, REPOS, key};
use north_graph::{Telling, Term};
use serde_json::Value;

use crate::boxes;
use crate::facts::{Facts, keyword, text};
use crate::places::Places;
use crate::shas::Shas;

const WRITER: &str = "mirror";
const REPLACED: [&str; 9] = ["title", "state", "label", "blocked-by", "has-box", "text", "ticked", "owner", "claims"];

pub fn read(places: &Places, shas: &Shas) -> Result<Telling, String> {
    let mut facts = Facts::new(&REPLACED);
    let mut seen = false;
    for repo in REPOS {
        let Some(open) = places.mirror.read(&key(repo, OPEN_ISSUES)).and_then(|copy| copy.body.as_array().cloned()) else { continue };
        seen = true;
        for issue in &open {
            tell_issue(&mut facts, shas, repo, issue);
        }
        let closed = places.mirror.read(&key(repo, CLOSED_ISSUES)).map(|copy| copy.body["numbers"].clone());
        for number in closed.iter().filter_map(Value::as_array).flatten().filter_map(Value::as_u64) {
            facts.tell(WRITER, text(format!("{repo}#{number}")), "state", keyword("closed"));
        }
    }
    if seen { Ok(facts.finish()) } else { Err(format!("the mirror {} holds no open issues", places.mirror.path().display())) }
}

fn tell_issue(facts: &mut Facts, shas: &Shas, repo: &str, issue: &Value) {
    let Some(number) = issue["number"].as_u64() else { return };
    let name = format!("{repo}#{number}");
    facts.tell(WRITER, text(&name), "state", keyword("open"));
    if let Some(title) = issue["title"].as_str() {
        facts.tell(WRITER, text(&name), "title", text(title));
    }
    for label in issue["labels"]["nodes"].as_array().into_iter().flatten().filter_map(|label| label["name"].as_str()) {
        facts.tell(WRITER, text(&name), "label", text(label));
    }
    for blocker in issue["blockedBy"]["nodes"].as_array().into_iter().flatten() {
        if let (Some(other), Some(blocked)) = (blocker["number"].as_u64(), blocker["repository"]["name"].as_str()) {
            facts.tell(WRITER, text(&name), "blocked-by", text(format!("{blocked}#{other}")));
        }
    }
    for item in boxes::parse(issue["body"].as_str().unwrap_or_default()) {
        tell_box(facts, shas, &name, &item);
    }
}

fn tell_box(facts: &mut Facts, shas: &Shas, issue: &str, item: &boxes::Item) {
    let node = item.id(issue);
    facts.tell(WRITER, text(issue), "has-box", text(&node));
    facts.tell(WRITER, text(&node), "text", text(&item.text));
    facts.tell(WRITER, text(&node), "ticked", Term::boolean(item.ticked));
    let Some(owner) = &item.owner else { return };
    facts.tell(WRITER, text(&node), "owner", text(owner));
    if !item.ticked {
        return;
    }
    let landed: Vec<Term> = item.landed.iter().map(|sha| text(shas.resolve(sha))).collect();
    let done = |commit: Term| Term::triple(text(&node), keyword("done-at"), commit);
    if landed.is_empty() {
        facts.tell(owner, text(owner), "claims", done(keyword("none")));
    }
    for commit in landed {
        facts.tell(owner, text(owner), "claims", done(commit));
    }
}
