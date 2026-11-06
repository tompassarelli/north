use serde_json::{Value, json};

use crate::store::{Mirror, OWNER, key};
use crate::sync::{Answer, Fetch, GitHub, REFETCH_S};

pub const PAGES: &str = "pages/source";
pub const ACCOUNT_REPOS: &str = "account/repos";
pub const DEFAULT_BRANCH: &str = "main";
const COMPARES: &str = "compare/";
const COMPARE_BATCH: usize = 20;
const REPO_FIELDS: &str = "name,isArchived,isFork,pushedAt,defaultBranchRef,parent";

pub fn compare_kind(branch: &str) -> String {
    format!("{COMPARES}{branch}")
}

fn tip<'a>(nodes: &'a [Value], branch: &str) -> Option<&'a str> {
    nodes.iter().find(|node| node["name"] == branch)?["target"]["oid"].as_str()
}

pub fn fetch_pages(github: &dyn GitHub, repo: &str) -> Result<Value, String> {
    pages_answer(github.get(&format!("repos/{OWNER}/{repo}/pages"), None), repo)
}

pub(crate) fn pages_answer(answer: Result<Answer, String>, repo: &str) -> Result<Value, String> {
    match answer {
        Ok(Answer::Changed { body, .. }) => Ok(body),
        Ok(Answer::Unchanged) => Err(format!("GitHub answered 304 to an unconditional Pages read of {repo}")),
        Err(why) if why.contains("HTTP 404") => Ok(Value::Null),
        Err(why) => Err(why),
    }
}

pub fn fetch_repo_list() -> Result<Value, String> {
    crate::platform::github::json(&["repo", "list", OWNER, "--limit", "1000", "--json", REPO_FIELDS])
}

pub fn fetch_compares(repo: &str, branches: &[String]) -> Result<Vec<Value>, String> {
    let mut found = Vec::new();
    for chunk in branches.chunks(COMPARE_BATCH) {
        let declared: String = (0..chunk.len()).map(|n| format!(", $h{n}: String!")).collect();
        let aliased: String = (0..chunk.len()).map(|n| format!(" c{n}: compare(headRef: $h{n}) {{ aheadBy commits(first: 100) {{ nodes {{ message }} }} }}")).collect();
        let query = format!("query=query($owner: String!, $name: String!{declared}) {{ repository(owner: $owner, name: $name) {{ ref(qualifiedName: \"refs/heads/{DEFAULT_BRANCH}\") {{{aliased} }} }} }}");
        let (owner, name) = (format!("owner={OWNER}"), format!("name={repo}"));
        let heads: Vec<String> = chunk.iter().enumerate().map(|(n, branch)| format!("h{n}={branch}")).collect();
        let mut args = vec!["api", "graphql", "-f", &query, "-F", &owner, "-F", &name];
        args.extend(heads.iter().flat_map(|head| ["-f", head.as_str()]));
        let answer = crate::platform::github::json(&args)?;
        let base = &answer["data"]["repository"]["ref"];
        if answer.get("errors").is_some() || base.is_null() {
            return Err(format!("GitHub compare page for {repo} is incomplete"));
        }
        found.extend((0..chunk.len()).map(|n| {
            let one = &base[format!("c{n}")];
            let messages: String = one["commits"]["nodes"].as_array().into_iter().flatten().filter_map(|commit| commit["message"].as_str()).map(|message| format!("{message}\n")).collect();
            if one.is_null() { Value::Null } else { json!({"ahead_by": one["aheadBy"], "messages": messages}) }
        }));
    }
    Ok(found)
}

pub fn periodic(mirror: &Mirror, key: &str, at: f64, fetch: impl FnOnce() -> Result<Value, String>) -> Result<Fetch, String> {
    if mirror.read(key).is_some_and(|copy| at - copy.fetched < REFETCH_S) {
        return mirror.confirm(key, at).map(|()| Fetch::Unchanged);
    }
    mirror.write(key, &fetch()?, None, at).map(|()| Fetch::Refreshed)
}

pub fn compares(github: &dyn GitHub, mirror: &Mirror, repo: &str, at: f64, nodes: &[Value]) -> Result<(), String> {
    let base = tip(nodes, DEFAULT_BRANCH).unwrap_or_default();
    let heads: Vec<(&str, &str)> = nodes.iter().filter_map(|node| Some((node["name"].as_str().filter(|name| *name != DEFAULT_BRANCH)?, node["target"]["oid"].as_str()?))).collect();
    let current = |branch: &str, head: &str| crate::read::compared(mirror, repo, branch, base, head).is_some();
    let stale: Vec<String> = heads.iter().filter(|(branch, head)| !current(branch, head)).map(|(branch, _)| (*branch).to_owned()).collect();
    let answers = if stale.is_empty() { Vec::new() } else { github.compares(repo, &stale)? };
    for (branch, answer) in stale.iter().zip(&answers).filter(|(_, answer)| !answer.is_null()) {
        let head = heads.iter().find(|(name, _)| name == branch).map_or("", |(_, head)| head);
        mirror.write(&key(repo, &compare_kind(branch)), &json!({"base": base, "head": head, "ahead_by": answer["ahead_by"], "messages": answer["messages"]}), None, at)?;
    }
    let kept: Vec<String> = heads.iter().map(|(branch, _)| key(repo, &compare_kind(branch))).collect();
    mirror.forget_except(&key(repo, COMPARES), &kept)
}

pub fn account(github: &dyn GitHub, mirror: &Mirror, at: f64) -> (String, Result<Fetch, String>) {
    (ACCOUNT_REPOS.to_owned(), periodic(mirror, ACCOUNT_REPOS, at, || github.repo_list()))
}

pub fn pages(github: &dyn GitHub, mirror: &Mirror, repo: &str, at: f64) -> (String, Result<Fetch, String>) {
    let pages = key(repo, PAGES);
    let fetched = periodic(mirror, &pages, at, || github.pages(repo));
    (pages, fetched)
}


pub fn live_pages(repo: &str) -> Result<Value, String> {
    fetch_pages(&crate::sync::Gh, repo)
}

pub fn live_repo_list() -> Result<Value, String> {
    fetch_repo_list()
}

pub fn live_compare(repo: &str, branch: &str) -> Result<Value, String> {
    let answers = fetch_compares(repo, &[branch.to_owned()])?;
    answers.into_iter().next().filter(|answer| !answer.is_null()).ok_or_else(|| format!("compare {branch} failed: GitHub has no such branch"))
}
