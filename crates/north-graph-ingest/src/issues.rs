use std::collections::BTreeMap;

use north_github_mirror::store::{CLOSED_ISSUES, OPEN_ISSUES, REPOS, key};
use north_graph::Term;
use serde_json::Value;

use crate::boxes::{self, Item};
use crate::facts::{Facts, Sourced, keyword, text};
use crate::places::Places;
use crate::shas::Shas;
use crate::waits::{self, Wait};

const WRITER: &str = "mirror";
const REPLACED: [&str; 12] = ["title", "state", "label", "blocked-by", "follows", "has-box", "text", "ticked", "owner", "claims", "waits-on", "unparsed-wait"];

fn open_issues(places: &Places, repo: &str) -> Option<Vec<Value>> {
    places.mirror.read(&key(repo, OPEN_ISSUES)).and_then(|copy| copy.body.as_array().cloned())
}

pub fn bodies(places: &Places) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    for repo in REPOS {
        for issue in open_issues(places, repo).iter().flatten() {
            if let Some(number) = issue["number"].as_u64() {
                found.insert(format!("{repo}#{number}"), issue["body"].as_str().unwrap_or_default().to_owned());
            }
        }
    }
    found
}

pub fn read(places: &Places, shas: &Shas) -> Result<Sourced, String> {
    let mut facts = Facts::new(&REPLACED);
    let mut seen = false;
    for repo in REPOS {
        let Some(open) = open_issues(places, repo) else { continue };
        seen = true;
        for issue in &open {
            tell_issue(&mut facts, shas, repo, issue);
        }
        let closed = places.mirror.read(&key(repo, CLOSED_ISSUES)).map(|copy| copy.body["numbers"].clone());
        for number in closed.iter().filter_map(Value::as_array).flatten().filter_map(Value::as_u64) {
            facts.tell(WRITER, text(format!("{repo}#{number}")), "state", keyword("closed"));
        }
    }
    if seen { Ok(facts.finish_noted()) } else { Err(format!("the mirror {} holds no open issues", places.mirror.path().display())) }
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
    let body = issue["body"].as_str().unwrap_or_default();
    let (follows, unparsed) = waits::follows(body, repo);
    for other in follows.iter().filter(|other| **other != name) {
        facts.tell(WRITER, text(&name), "follows", text(other));
    }
    for line in unparsed {
        tell_unparsed(facts, &name, &name, &line);
    }
    let items = boxes::parse(body);
    for item in &items {
        tell_box(facts, shas, &name, item, &items);
    }
}

fn tell_unparsed(facts: &mut Facts, subject: &str, issue: &str, phrase: &str) {
    facts.tell(WRITER, text(subject), "unparsed-wait", text(phrase));
    facts.note(format!("cannot parse a wait of {issue}: {phrase}"));
}

fn tell_box(facts: &mut Facts, shas: &Shas, issue: &str, item: &Item, siblings: &[Item]) {
    let node = item.id(issue);
    facts.tell(WRITER, text(issue), "has-box", text(&node));
    facts.tell(WRITER, text(&node), "text", text(&item.text));
    facts.tell(WRITER, text(&node), "ticked", Term::boolean(item.ticked));
    if !item.ticked {
        let waits = waits::of(item, issue, siblings);
        for wait in waits.on {
            let target = match wait {
                Wait::Issue(name) => name,
                Wait::Box(id) => id,
            };
            facts.tell(WRITER, text(&node), "waits-on", text(target));
        }
        for phrase in waits.unparsed {
            tell_unparsed(facts, &node, issue, &format!("{} (box {})", phrase, item.head.chars().take(60).collect::<String>()));
        }
    }
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
