//! `north issue child`: one sub-issue under a parent, created with its type and labels and linked to the parent by one
//! GraphQL mutation sent as the gh shim's App token (north#1137).
use std::collections::BTreeMap;

use north_base::platform::github;
use serde_json::Value;

use super::issue_convert::{manifest, read, scan, slug};
use north_base::platform::box_oracles::done_when_command;

const USAGE: &str = "usage: north issue child [OWNER/]REPO#N --title TITLE --body BODY [--box TEXT] [--type Task|Bug|Feature] [--label LABEL]...

Creates one sub-issue under the parent. A confined worker names no parent: its own task issue is the parent, and the outside broker, which reads the host token the confined session cannot, runs the verb for it. --box TEXT is added to the body as one unticked box; with no --label the parent's type, priority and category labels are taken, and its Category: and Class: lines go first in a body that has none. Its type and labels are set and it is linked to the parent in one GraphQL mutation
with that token, and the new issue's URL is printed. It refuses a title equal to an open issue's, a body with no
runnable Done when (a command in backticks), a parent in another owner's repository or one the host's token does not cover (or whose token lacks issues:write), a parent that holds 100 children
already, a type or label the repository lacks, and text the public deny-list matches in a public repository.";

struct Asked {
    owner: String,
    name: String,
    number: u64,
    title: String,
    body: String,
    kind: String,
    labels: Vec<String>,
    box_text: Option<String>,
}

#[derive(Default)]
struct Tokens {
    parent: Option<String>,
    title: Option<String>,
    body: Option<String>,
    kind: Option<String>,
    labels: Vec<String>,
    box_text: Option<String>,
}

impl Tokens {
    fn flag(&mut self, flag: &str, value: String) -> Result<(), String> {
        match flag {
            "--title" => self.title = Some(value),
            "--body" => self.body = Some(value),
            "--type" => self.kind = Some(value),
            "--label" => self.labels.push(value),
            "--box" => self.box_text = Some(value),
            _ => return Err(format!("unknown flag {flag}")),
        }
        Ok(())
    }
}

fn tokens(arguments: &[String]) -> Result<Tokens, String> {
    let mut tokens = Tokens::default();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        if word.starts_with("--") {
            let value = words.next().ok_or_else(|| format!("{word} needs a value"))?.to_owned();
            tokens.flag(word, value)?;
        } else if tokens.parent.replace(word.clone()).is_some() {
            return Err(format!("{word} is a second parent"));
        }
    }
    Ok(tokens)
}

fn parent(text: &str) -> Result<(String, String, u64), String> {
    let (repository, number) = text.split_once('#').ok_or_else(|| format!("{text} is not [OWNER/]REPO#N"))?;
    let number = number.parse::<u64>().map_err(|_| format!("{text} is not [OWNER/]REPO#N"))?;
    let (owner, name) = slug(repository)?;
    Ok((owner, name, number))
}

fn asked(arguments: &[String]) -> Result<Asked, String> {
    let tokens = tokens(arguments)?;
    let (owner, name, number) = parent(&tokens.parent.ok_or("name the parent as [OWNER/]REPO#N")?)?;
    let title = tokens.title.filter(|title| !title.trim().is_empty()).ok_or("--title names the child's title")?;
    let body = tokens.body.ok_or("--body names the child's body")?;
    Ok(Asked { owner, name, number, title, body, kind: tokens.kind.unwrap_or_else(|| "Task".to_owned()), labels: tokens.labels, box_text: tokens.box_text })
}

pub fn run(arguments: &[String]) -> u8 {
    if matches!(arguments, [word] if word == "-h" || word == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let asked = match asked(arguments) {
        Ok(asked) => asked,
        Err(why) => {
            eprintln!("north issue child: {why}\n{USAGE}");
            return 2;
        }
    };
    match create(&asked) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(why) => {
            eprintln!("north issue child: {why}");
            1
        }
    }
}

fn create(asked: &Asked) -> Result<bool, String> {
    let reference = format!("{}/{}#{}", asked.owner, asked.name, asked.number);
    let open = read::repository(&asked.owner, &asked.name, false)?;
    let target = read::target(&asked.owner, &asked.name, asked.number)?;
    let completed = completed(asked, open.issues.iter().find(|issue| issue.number == asked.number), &target);
    let asked = &completed;
    let titles: Vec<String> = open.issues.iter().map(|issue| issue.title.clone()).collect();
    let scanner = scan(open.private);
    let mut refused = refusals(asked, &titles, &target, &*scanner);
    refused.extend(held_scope_refusal(&asked.name));
    if !refused.is_empty() {
        refused.iter().for_each(|why| println!("refused: {why}"));
        return Ok(false);
    }
    let answer = create_issue(asked, &target)?;
    println!("created {}/{}#{} under {reference}: {}", asked.owner, asked.name, answer["number"], answer["url"].as_str().unwrap_or_default());
    Ok(true)
}

const FIRST_LINES: [&str; 2] = ["Category", "Class"];
const INHERITED_LABELS: [&str; 3] = ["bug", "enhancement", "documentation"];

fn completed(asked: &Asked, parent: Option<&read::Issue>, target: &read::Target) -> Asked {
    let mut body = asked.body.clone();
    let mut labels = asked.labels.clone();
    if let Some(parent) = parent {
        let present = |key: &str| body.lines().any(|line| line.trim_start_matches(|letter: char| letter.is_whitespace() || "*>-".contains(letter)).to_ascii_lowercase().starts_with(&format!("{}:", key.to_ascii_lowercase())));
        let header: Vec<&str> = FIRST_LINES.iter().filter(|key| !present(key)).filter_map(|key| parent.body.lines().find(|line| line.starts_with(&format!("{key}:")))).collect();
        if !header.is_empty() {
            body = format!("{}\n\n{body}", header.join("\n"));
        }
        if labels.is_empty() {
            let inherited = |label: &&String| (label.starts_with("priority:") || label.starts_with("category:") || INHERITED_LABELS.contains(&label.as_str())) && target.labels.contains_key(*label);
            labels = parent.labels.iter().filter(inherited).cloned().collect();
        }
    }
    if let Some(text) = asked.box_text.as_deref().map(str::trim).filter(|text| !text.is_empty()) {
        body = format!("{}\n\n- [ ] {text}", body.trim_end());
    }
    Asked { owner: asked.owner.clone(), name: asked.name.clone(), number: asked.number, title: asked.title.clone(), body, kind: asked.kind.clone(), labels, box_text: None }
}

fn refusals(asked: &Asked, titles: &[String], target: &read::Target, scanner: &manifest::Scan) -> Vec<String> {
    let mut refused = Vec::new();
    if asked.owner != north_github_mirror::store::OWNER {
        refused.push(format!("the parent is in {}/{}, and a child is created only in a repository of {}", asked.owner, asked.name, north_github_mirror::store::OWNER));
    }
    let key = manifest::title_key(&asked.title);
    if titles.iter().any(|title| manifest::title_key(title) == key) {
        refused.push("its title equals an open issue's".to_owned());
    }
    if done_when_command(&asked.body).is_none() {
        refused.push("its body has no runnable Done when (a command in backticks)".to_owned());
    }
    if target.children >= manifest::MOST_CHILDREN {
        refused.push(format!("the parent holds {} children and GitHub allows {} under one parent", target.children, manifest::MOST_CHILDREN));
    }
    refused.extend(asked.labels.iter().filter(|label| !target.labels.contains_key(*label)).map(|label| format!("the repository has no label {label}")));
    if !target.issue_types.contains_key(&asked.kind) {
        refused.push(format!("the repository has no type {}", asked.kind));
    }
    match scanner(&format!("{}\n\n{}", asked.title, asked.body)) {
        Ok(rules) if rules.is_empty() => {}
        Ok(rules) => refused.push(format!("the privacy scan refuses it (deny-list rule {})", rules.into_iter().collect::<Vec<_>>().join(", "))),
        Err(why) => refused.push(format!("the privacy scan could not read it, so it is refused: {why}")),
    }
    refused
}

const CREATE: &str = "mutation($input: CreateIssueInput!) { createIssue(input: $input) { issue { number url } } }";
const HELD_TOKEN: &str = ".local/state/north/github-installation-token.yaml";

/// The one GraphQL mutation that creates the child and links it to its parent, sent as the gh shim's App token.
fn create_issue(asked: &Asked, target: &read::Target) -> Result<Value, String> {
    let issue_type_id = target.issue_types.get(&asked.kind).cloned().unwrap_or_default();
    let mut fields: Vec<String> = [("repositoryId", &target.repository_id), ("parentIssueId", &target.parent_id), ("issueTypeId", &issue_type_id), ("title", &asked.title), ("body", &asked.body)].map(|(key, value)| format!("input[{key}]={value}")).to_vec();
    fields.extend(asked.labels.iter().map(|label| format!("input[labelIds][]={}", target.labels[label])));
    let query = format!("query={CREATE}");
    let mut arguments = vec!["api", "graphql", "-f", query.as_str()];
    fields.iter().for_each(|field| arguments.extend(["-f", field.as_str()]));
    let answer = github::json(&arguments)?;
    let issue = &answer["data"]["createIssue"]["issue"];
    if issue.is_object() { Ok(issue.clone()) } else { Err(format!("GitHub created no issue: {}", answer["errors"])) }
}

/// Why the token this host holds cannot create an issue in `name`: the repositories and permissions the laptop sent it with, the same ceiling the token broker applied to a child (none when this host holds no token file, whose mint is then the broker's to limit).
fn held_scope_refusal(name: &str) -> Option<String> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;
    let text = std::fs::read_to_string(std::path::Path::new(&home).join(HELD_TOKEN)).ok()?;
    let fields: BTreeMap<&str, &str> = text.lines().filter_map(|line| line.split_once(':')).map(|(key, value)| (key.trim(), value.trim())).collect();
    let list = |key: &str| fields.get(key).map(|value| value.split(',').map(str::trim).filter(|word| !word.is_empty()).collect::<Vec<_>>()).unwrap_or_default();
    if !list("repositories").contains(&name) {
        return Some(format!("{name} is outside the repositories this host's token covers ({})", list("repositories").join(", ")));
    }
    (!list("permissions").iter().any(|pair| matches!(pair.split_once(':'), Some(("issues", "write" | "admin"))))).then(|| "this host's token holds no issues:write".to_owned())
}
