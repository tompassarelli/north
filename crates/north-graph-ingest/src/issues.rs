use std::collections::BTreeMap;

use north_github_mirror::store::{CLOSED_ISSUES, OPEN_ISSUES, REPOS, key};
use north_graph::Term;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::boxes::{self, Item};
use crate::facts::{Facts, Sourced, issue_ref, keyword, text};
use crate::places::Places;
use crate::shas::Shas;
use crate::waits::{self, Wait};

const WRITER: &str = "mirror";
const RUNS: &str = "ledger";
const REPLACED: [&str; 15] = ["title", "state", "label", "blocked-by", "follows", "has-box", "text", "ticked", "owner", "claims", "waits-on", "unparsed-wait", "parent", "leaf", "done-when"];

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
    run_claims(&mut facts, places, shas);
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
    let parent = issue["parent"]["number"].as_u64().zip(issue["parent"]["repository"]["name"].as_str());
    if let Some((other, parent_repo)) = parent {
        facts.tell(WRITER, text(&name), "parent", text(format!("{parent_repo}#{other}")));
    }
    let items = boxes::parse(body);
    for item in &items {
        tell_box(facts, shas, &name, item, &items);
    }
    let open_children = issue["subIssuesSummary"]["total"].as_u64().unwrap_or(0).saturating_sub(issue["subIssuesSummary"]["completed"].as_u64().unwrap_or(0));
    if parent.is_some() && items.is_empty() && open_children == 0 {
        facts.tell(WRITER, text(&name), "leaf", Term::boolean(true));
        if let Some(command) = boxes::done_when(body) {
            facts.tell(WRITER, text(&name), "done-when", text(command));
        }
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
    if let Some(command) = &item.done_when {
        facts.tell(WRITER, text(&node), "done-when", text(command));
    }
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

fn run_claims(facts: &mut Facts, places: &Places, shas: &Shas) {
    if let Err(why) = told_runs(facts, places, shas) {
        facts.note(format!("no run claims from {}: {why}", places.threads.display()));
    }
}

fn told_runs(facts: &mut Facts, places: &Places, shas: &Shas) -> rusqlite::Result<()> {
    if !places.threads.is_file() {
        return Ok(());
    }
    let connection = Connection::open_with_flags(&places.threads, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let query = format!("SELECT agent, item, box, landed_sha FROM runs WHERE outcome = 'done' AND item IS NOT NULL AND box IS NOT NULL AND box != '' AND {}", north_base::platform::run_roles::worker_where());
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?)))?;
    for row in rows {
        let (agent, item, box_text, landed) = row?;
        let Some(node) = issue_ref(&item).and_then(|issue| north_threads::threads::claims::box_node(&issue, &box_text)) else { continue };
        if let Some(command) = boxes::done_when(&box_text) {
            facts.tell(RUNS, text(&node), "done-when", text(command));
        }
        let commit = landed.filter(|sha| !sha.is_empty()).map_or_else(|| keyword("none"), |sha| text(shas.resolve(&sha)));
        facts.tell(&agent, text(&agent), "claims", Term::triple(text(&node), keyword("done-at"), commit));
    }
    Ok(())
}
