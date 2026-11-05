use serde_json::{Value, json};

use crate::store::{self, BRANCHES, BRANCHES_PROBE, CLOSED_ISSUES, ISSUES_PROBE, MAIN_CHECKS, MAIN_RUNS, Mirror, OPEN_ISSUES, OWNER, RELEASES, TAGS, key};

pub const REFETCH_S: f64 = 30.0 * 60.0;

#[derive(Debug, PartialEq)]
pub enum Answer {
    Unchanged,
    Changed { body: Value, etag: Option<String> },
}

pub trait GitHub {
    fn get(&self, endpoint: &str, etag: Option<&str>) -> Result<Answer, String>;
    fn open_issues(&self, repo: &str) -> Result<Vec<Value>, String>;
    fn closed_issues(&self, repo: &str) -> Result<Vec<Value>, String>;
    fn branches(&self, repo: &str) -> Result<Vec<Value>, String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch {
    Unchanged,
    Refreshed,
}

pub struct Gh;

impl GitHub for Gh {
    fn get(&self, endpoint: &str, etag: Option<&str>) -> Result<Answer, String> {
        let mut args = vec!["api".to_owned(), "--include".to_owned()];
        if let Some(etag) = etag {
            args.extend(["-H".to_owned(), format!("If-None-Match: {etag}")]);
        }
        args.push(endpoint.to_owned());
        let out = crate::platform::github::run_owned(&args).map_err(|error| format!("cannot run gh: {error}"))?;
        response(&String::from_utf8_lossy(&out.stdout)).map_err(|why| format!("gh api {endpoint}: {why} {}", String::from_utf8_lossy(&out.stderr).trim()))
    }

    fn open_issues(&self, repo: &str) -> Result<Vec<Value>, String> {
        store::fetch_open_issues(repo)
    }

    fn closed_issues(&self, repo: &str) -> Result<Vec<Value>, String> {
        store::fetch_closed_issue_numbers(repo)
    }

    fn branches(&self, repo: &str) -> Result<Vec<Value>, String> {
        store::fetch_branches(repo)
    }
}

pub fn response(text: &str) -> Result<Answer, String> {
    let mut lines = text.split_inclusive('\n');
    let status = lines.next().and_then(|line| line.split_whitespace().nth(1)).and_then(|code| code.parse::<u16>().ok()).ok_or("no HTTP status line")?;
    let mut etag = None;
    let mut consumed = text.split_inclusive('\n').next().map_or(0, str::len);
    for line in lines {
        consumed += line.len();
        if line.trim().is_empty() {
            break;
        }
        if let Some((_, value)) = line.split_once(':').filter(|(name, _)| name.trim().eq_ignore_ascii_case("etag")) {
            etag = Some(value.trim().to_owned());
        }
    }
    let body = &text[consumed.min(text.len())..];
    match status {
        304 => Ok(Answer::Unchanged),
        200..=299 => serde_json::from_str(body).map(|body| Answer::Changed { body, etag }).map_err(|error| format!("HTTP {status} body is not JSON: {error}")),
        _ => Err(format!("HTTP {status}")),
    }
}

struct Asked {
    changed: Option<(Value, Option<String>)>,
}

fn ask(github: &dyn GitHub, mirror: &Mirror, key: &str, endpoint: &str) -> Result<Asked, String> {
    let held = mirror.read(key);
    match github.get(endpoint, held.as_ref().and_then(|copy| copy.etag.as_deref()))? {
        Answer::Unchanged if held.is_some() => Ok(Asked { changed: None }),
        Answer::Unchanged => Err(format!("GitHub answered 304 for {key}, which the mirror does not hold")),
        Answer::Changed { body, etag } if held.as_ref().is_some_and(|copy| copy.body == body) => {
            mirror.write(key, &body, etag.as_deref(), held.map_or(0.0, |copy| copy.fetched))?;
            Ok(Asked { changed: None })
        }
        Answer::Changed { body, etag } => Ok(Asked { changed: Some((body, etag)) }),
    }
}

fn save(mirror: &Mirror, key: &str, asked: Asked, at: f64) -> Result<Fetch, String> {
    match asked.changed {
        None => mirror.confirm(key, at).map(|()| Fetch::Unchanged),
        Some((body, etag)) => mirror.write(key, &body, etag.as_deref(), at).map(|()| Fetch::Refreshed),
    }
}

fn conditional(github: &dyn GitHub, mirror: &Mirror, key: &str, endpoint: &str, at: f64) -> Result<Fetch, String> {
    let asked = ask(github, mirror, key, endpoint)?;
    save(mirror, key, asked, at)
}

struct Listing<'a> {
    probe: &'a str,
    endpoint: String,
    kind: &'a str,
    also: &'a [&'a str],
}

fn probed(
    github: &dyn GitHub,
    mirror: &Mirror,
    repo: &str,
    at: f64,
    listing: &Listing,
    fetch: impl FnOnce() -> Result<Vec<Value>, String>,
    then: impl FnOnce(&[Value]) -> Result<(), String>,
) -> Result<Fetch, String> {
    let probe = key(repo, listing.probe);
    let asked = ask(github, mirror, &probe, &listing.endpoint)?;
    let body = key(repo, listing.kind);
    let current = mirror.read(&body).is_some_and(|copy| at - copy.fetched < REFETCH_S);
    if asked.changed.is_none() && current {
        for kind in [listing.kind].iter().chain(listing.also) {
            mirror.confirm(&key(repo, kind), at)?;
        }
        return save(mirror, &probe, asked, at);
    }
    let nodes = Value::Array(fetch()?);
    mirror.write(&body, &nodes, None, at)?;
    then(nodes.as_array().map_or(&[], Vec::as_slice))?;
    save(mirror, &probe, asked, at)?;
    Ok(Fetch::Refreshed)
}

fn closed_numbers(github: &dyn GitHub, mirror: &Mirror, repo: &str, at: f64, open: &[Value]) -> Result<(), String> {
    let (closed, open) = (key(repo, CLOSED_ISSUES), Value::Array(open.iter().map(|issue| issue["number"].clone()).collect()));
    if mirror.read(&closed).is_some_and(|copy| copy.body["open"] == open) {
        return mirror.confirm(&closed, at);
    }
    let numbers = github.closed_issues(repo)?;
    mirror.write(&closed, &json!({"open": open, "numbers": numbers}), None, at)
}

pub fn issues(github: &dyn GitHub, mirror: &Mirror, repo: &str, at: f64) -> Result<Fetch, String> {
    let listing = Listing { probe: ISSUES_PROBE, endpoint: format!("repos/{OWNER}/{repo}/issues?state=all&sort=updated&direction=desc&per_page=1"), kind: OPEN_ISSUES, also: &[CLOSED_ISSUES] };
    probed(github, mirror, repo, at, &listing, || github.open_issues(repo), |open| closed_numbers(github, mirror, repo, at, open))
}

fn branches(github: &dyn GitHub, mirror: &Mirror, repo: &str, at: f64) -> Result<Fetch, String> {
    let listing = Listing { probe: BRANCHES_PROBE, endpoint: format!("repos/{OWNER}/{repo}/git/matching-refs/heads?per_page=100"), kind: BRANCHES, also: &[] };
    probed(github, mirror, repo, at, &listing, || github.branches(repo), |_| Ok(()))
}

pub fn repository(github: &dyn GitHub, mirror: &Mirror, repo: &str, at: f64) -> Vec<(String, Result<Fetch, String>)> {
    let listed = |kind: &str, endpoint: String| (key(repo, kind), conditional(github, mirror, &key(repo, kind), &format!("repos/{OWNER}/{repo}/{endpoint}"), at));
    vec![
        (key(repo, OPEN_ISSUES), issues(github, mirror, repo, at)),
        (key(repo, BRANCHES), branches(github, mirror, repo, at)),
        listed(MAIN_RUNS, "actions/runs?branch=main&per_page=60".into()),
        listed(MAIN_CHECKS, "commits/main/check-runs?per_page=100".into()),
        listed(TAGS, "git/matching-refs/tags?per_page=100".into()),
        listed(RELEASES, "releases?per_page=100".into()),
    ]
}

const USAGE: &str = "usage: north github-mirror sync [--repo NAME]...";

pub fn run(arguments: &[String]) -> u8 {
    let Some((_, rest)) = arguments.split_first().filter(|(verb, _)| verb.as_str() == "sync") else {
        eprintln!("{USAGE}");
        return 2;
    };
    let mut repos = Vec::new();
    let mut rest = rest.iter();
    while let Some(argument) = rest.next() {
        match (argument.as_str(), rest.next()) {
            ("--repo", Some(name)) => repos.push(name.clone()),
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    if repos.is_empty() {
        repos = store::REPOS.iter().map(|repo| (*repo).to_owned()).collect();
    }
    let (mirror, at) = (Mirror::local(), crate::foundation::clock::Timestamp::now().unix_seconds_f64());
    let mut failed = false;
    for (key, result) in repos.iter().flat_map(|repo| repository(&Gh, &mirror, repo, at)) {
        match result {
            Ok(Fetch::Unchanged) => println!("{key} unchanged"),
            Ok(Fetch::Refreshed) => println!("{key} refreshed"),
            Err(why) => {
                failed = true;
                println!("{key} failed: {why}");
            }
        }
    }
    u8::from(failed)
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
