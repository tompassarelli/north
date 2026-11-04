use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;

use super::{Detector, Finding, Limits};

static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+[a-z]?(?:, \d+[a-z]?)*)\. \*\*([^*]+)\*\*").expect("plan item pattern"));
static OWNER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^Owner: (\S+#\d+)").expect("owner pattern"));
static BOX_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+[a-z]?$").expect("box id pattern"));

pub struct Doc {
    pub slug: String,
    pub added_at: f64,
    pub text: String,
}

pub struct Landed {
    pub at: f64,
    pub subject: String,
}

pub struct IssueBox {
    pub reference: String,
    pub title: String,
    pub ready_since: f64,
}

#[derive(Default)]
pub struct Facts {
    pub docs: Vec<Doc>,
    pub landed: Vec<Landed>,
    pub names: Vec<String>,
    pub issues: Vec<IssueBox>,
}

#[derive(Debug, Eq, PartialEq)]
struct PlanBox {
    id: String,
    title: String,
    marked_landed: bool,
    after: Vec<String>,
}

fn tokens(text: &str) -> BTreeSet<String> {
    text.to_lowercase().split(|character: char| !character.is_ascii_alphanumeric()).filter(|token| !token.is_empty()).map(str::to_owned).collect()
}

fn runs_after(rows: &BTreeMap<String, String>, id: &str, known: &BTreeSet<String>) -> Vec<String> {
    let cell = rows.get(id).map_or("", String::as_str);
    let cell = cell.split("in the order").next().unwrap_or_default();
    cell.split(|character: char| !character.is_ascii_alphanumeric()).filter(|token| BOX_ID.is_match(token) && known.contains(*token) && *token != id).map(str::to_owned).collect()
}

fn table(text: &str) -> BTreeMap<String, String> {
    let mut rows = BTreeMap::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("| Box |") {
            inside = true;
            continue;
        }
        if inside && !line.starts_with('|') {
            break;
        }
        if inside && !line.starts_with("| ---") && !line.starts_with("|---") {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
            if let [ids, _, after] = cells[..] {
                for id in ids.split(", ").filter(|id| BOX_ID.is_match(id)) {
                    rows.insert(id.to_owned(), after.to_owned());
                }
            }
        }
    }
    rows
}

fn boxes(text: &str) -> Vec<PlanBox> {
    let mut items: Vec<(Vec<String>, String, String)> = Vec::new();
    for line in text.lines() {
        if let Some(found) = ITEM.captures(line) {
            let ids = found[1].split(", ").map(str::to_owned).collect();
            items.push((ids, found[2].trim().trim_end_matches('.').to_owned(), line.to_owned()));
        } else if line.starts_with('#') {
            items.push((Vec::new(), String::new(), String::new()));
        } else if let Some(last) = items.last_mut() {
            last.2.push('\n');
            last.2.push_str(line);
        }
    }
    let items: Vec<_> = items.into_iter().filter(|(ids, _, body)| !ids.is_empty() && body.split_whitespace().collect::<Vec<_>>().join(" ").contains("Done when")).collect();
    let known: BTreeSet<String> = items.iter().flat_map(|(ids, _, _)| ids.iter().cloned()).collect();
    let rows = table(text);
    items
        .into_iter()
        .flat_map(|(ids, title, _)| {
            let marked_landed = title.to_lowercase().contains("landed");
            ids.into_iter().map(|id| PlanBox { after: runs_after(&rows, &id, &known), id, title: title.clone(), marked_landed }).collect::<Vec<_>>()
        })
        .collect()
}

fn landed_at(doc: &Doc, plan_box: &PlanBox, landed: &[Landed]) -> Option<f64> {
    if plan_box.marked_landed {
        return Some(doc.added_at);
    }
    let needle = format!("{} box {}", doc.slug.to_lowercase(), plan_box.id);
    landed
        .iter()
        .filter(|commit| {
            let subject = commit.subject.to_lowercase();
            subject.match_indices(&needle).any(|(start, _)| !subject[start + needle.len()..].starts_with(|next: char| next.is_ascii_alphanumeric()))
        })
        .map(|commit| commit.at)
        .reduce(f64::min)
}

fn names_box(names: &[String], slug: &str, id: &str) -> bool {
    let slug = slug.to_lowercase();
    let slug_tokens = tokens(&slug);
    names.iter().any(|name| {
        let found = tokens(name);
        found.contains(id) && (name.to_lowercase().contains(&slug) || found.iter().any(|token| token.len() >= 3 && slug_tokens.contains(token)))
    })
}

fn names_issue(names: &[String], reference: &str) -> bool {
    let (_, number) = reference.rsplit_once('#').unwrap_or_default();
    names.iter().any(|name| {
        name.to_lowercase().contains(reference) || tokens(name).iter().any(|token| token.trim_start_matches(|character: char| character.is_ascii_alphabetic()) == number && token.len() - number.len() <= 3)
    })
}

fn owner(doc: &Doc) -> Option<String> {
    OWNER.captures(&doc.text).map(|found| found[1].to_owned())
}

fn plan_findings(doc: &Doc, facts: &Facts, now: f64, limits: &Limits) -> Vec<Finding> {
    let all = boxes(&doc.text);
    let landed: BTreeMap<&str, f64> = all.iter().filter_map(|plan_box| Some((plan_box.id.as_str(), landed_at(doc, plan_box, &facts.landed)?))).collect();
    all.iter()
        .filter(|plan_box| !landed.contains_key(plan_box.id.as_str()))
        .filter_map(|plan_box| {
            let ready_since = plan_box.after.iter().map(|id| landed.get(id.as_str()).copied()).try_fold(doc.added_at, |latest, at| Some(latest.max(at?)))?;
            if now - ready_since <= limits.stall_s || names_box(&facts.names, &doc.slug, &plan_box.id) {
                return None;
            }
            let hours = (now - ready_since) / 3600.0;
            Some(Finding {
                detector: Detector::StalledPlan,
                key: format!("plan:{}#{}", doc.slug, plan_box.id),
                owner: owner(doc),
                title: format!("Staff {} box {}: {}", doc.slug, plan_box.id, plan_box.title),
                detail: format!("Every prerequisite of box {} in docs/rationale/{}.md landed {hours:.1} h ago and no worker, queued follow or lane names it.", plan_box.id, doc.slug),
                done_when: format!("a lane for box {} of docs/rationale/{}.md lands on origin/main with a commit subject naming `{} box {}`, and `north flow detect --dry-run` no longer prints `plan:{}#{}`.", plan_box.id, doc.slug, doc.slug, plan_box.id, doc.slug, plan_box.id),
                instances: vec![format!("{}#{}", doc.slug, plan_box.id)],
            })
        })
        .collect()
}

pub fn stalled(facts: &Facts, now: f64, limits: &Limits) -> Vec<Finding> {
    let mut findings: Vec<Finding> = facts.docs.iter().flat_map(|doc| plan_findings(doc, facts, now, limits)).collect();
    let repos = super::issue_stall_repos();
    findings.extend(facts.issues.iter().filter(|issue| now - issue.ready_since > limits.stall_s && !names_issue(&facts.names, &issue.reference) && repos.iter().any(|repo| issue.reference.starts_with(&format!("{repo}#")))).map(|issue| Finding {
        detector: Detector::StalledPlan,
        key: format!("issue:{}", issue.reference),
        owner: Some(issue.reference.clone()),
        title: format!("Staff the unchecked box of {}", issue.reference),
        detail: format!("{} has had an unchecked box with every predecessor closed for {:.1} h and no worker, queued follow or lane names it.", issue.reference, (now - issue.ready_since) / 3600.0),
        done_when: format!("a lane for {} lands on origin/main, or `threads unowned` no longer lists {} as UNOWNED.", issue.reference, issue.reference),
        instances: vec![issue.reference.clone()],
    }));
    findings.sort_by(|a, b| a.key.cmp(&b.key));
    findings
}

#[cfg(test)]
pub(super) fn parsed(text: &str) -> Vec<(String, Vec<String>, bool)> {
    boxes(text).into_iter().map(|plan_box| (plan_box.id, plan_box.after, plan_box.marked_landed)).collect()
}
