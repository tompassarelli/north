//! `north issue child`: one sub-issue under a parent, created with its type and labels and linked to the parent by one
//! GraphQL mutation through the broker (north#1137).
use north_base::platform::broker_client::Client;
use serde_json::json;

use super::issue_convert::{manifest, read, scan, slug};
use crate::platform::box_oracles::done_when_command;

const USAGE: &str = "usage: north issue child [OWNER/]REPO#N --title TITLE --body BODY [--type Task|Bug|Feature] [--label LABEL]...

Creates one sub-issue under the parent. Its type and labels are set and it is linked to the parent in one GraphQL mutation
through the broker, and the new issue's URL is printed. It refuses a title equal to an open issue's, a body with no
runnable Done when (a command in backticks), a parent in another owner's repository, a parent that holds 100 children
already, a type or label the repository lacks, and text the public deny-list matches in a public repository.";

struct Asked {
    owner: String,
    name: String,
    number: u64,
    title: String,
    body: String,
    kind: String,
    labels: Vec<String>,
}

#[derive(Default)]
struct Tokens {
    parent: Option<String>,
    title: Option<String>,
    body: Option<String>,
    kind: Option<String>,
    labels: Vec<String>,
}

impl Tokens {
    fn flag(&mut self, flag: &str, value: String) -> Result<(), String> {
        match flag {
            "--title" => self.title = Some(value),
            "--body" => self.body = Some(value),
            "--type" => self.kind = Some(value),
            "--label" => self.labels.push(value),
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
    Ok(Asked { owner, name, number, title, body, kind: tokens.kind.unwrap_or_else(|| "Task".to_owned()), labels: tokens.labels })
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
    let titles: Vec<String> = open.issues.iter().map(|issue| issue.title.clone()).collect();
    let scanner = scan(open.private);
    let refused = refusals(asked, &titles, &target, &*scanner);
    if !refused.is_empty() {
        refused.iter().for_each(|why| println!("refused: {why}"));
        return Ok(false);
    }
    let issue_type_id = target.issue_types.get(&asked.kind).cloned().unwrap_or_default();
    let label_ids: Vec<String> = asked.labels.iter().map(|label| target.labels[label].clone()).collect();
    let answer = Client::discover().github_child(&json!({
        "repository": asked.name,
        "parent": asked.number,
        "issue_type_id": issue_type_id,
        "label_ids": label_ids,
        "title": asked.title,
        "body": asked.body,
    }))?;
    println!("created {}/{}#{} under {reference}: {}", asked.owner, asked.name, answer["number"], answer["url"].as_str().unwrap_or_default());
    Ok(true)
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
