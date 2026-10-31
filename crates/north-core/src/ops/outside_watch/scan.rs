use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use regex::Regex;
use serde_json::Value;

use super::{Config, Failure, Gh};
use crate::foundation::clock::Timestamp;

const YOUNG: i64 = 30 * 86_400;
const NO_STANDING: [&str; 3] = ["NONE", "FIRST_TIMER", "FIRST_TIME_CONTRIBUTOR"];
const TLDS: &str = "com|net|org|io|ai|dev|app|co|xyz|me|so|gg|tech|info|site|online|pro|cc|us|uk|de|fr|ru|cn|in|biz|shop|store|tv|ly|to";
const OURS_DOMAINS: [&str; 4] = ["github.com", "githubusercontent.com", "github.io", "githubassets.com"];
const QUOTE: &AsciiSet = &NON_ALPHANUMERIC.remove(b'_').remove(b'.').remove(b'-').remove(b'~').remove(b'/');
const DISCUSSIONS: &str = "query($q: String!) { search(type: DISCUSSION, query: $q, first: 50) { nodes { ... on Discussion {
  id number url title body createdAt authorAssociation author { login __typename } repository { name }
  comments(last: 50) { nodes { id url body createdAt authorAssociation author { login __typename }
    replies(last: 50) { nodes { id url body createdAt authorAssociation author { login __typename } } } } } } } } }";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Class {
    Spam,
    Real,
}

pub(super) struct Post {
    pub kind: &'static str,
    pub repo: String,
    pub number: i64,
    pub id: String,
    pub actor: String,
    pub is_user: bool,
    pub body: String,
    pub association: String,
    pub created: String,
    pub url: String,
}

fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}

fn domain_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(&format!(r"(?i)(?:https?://)?((?:[a-z0-9-]+\.)+(?:{TLDS}))(?:[^a-z0-9-]|$)")).unwrap())
}

pub(super) fn domains(body: &str) -> BTreeSet<String> {
    domain_pattern()
        .captures_iter(body)
        .map(|found| found[1].to_lowercase())
        .map(|domain| domain.strip_prefix("www.").map_or(domain.clone(), str::to_owned))
        .filter(|domain| !OURS_DOMAINS.iter().any(|ours| domain == ours || domain.ends_with(&format!(".{ours}"))))
        .collect()
}

fn hostname(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let netloc = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = netloc.rsplit_once('@').map_or(netloc, |(_, host)| host);
    let host = match host.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|byte| byte.is_ascii_digit()) => name,
        _ => host,
    };
    host.to_lowercase()
}

fn profile_domains(profile: &Value) -> BTreeSet<String> {
    let joined = ["blog", "bio", "company"].map(|key| profile.get(key).and_then(Value::as_str).unwrap_or("").to_owned()).join(" ");
    let blog = text(profile, "blog");
    let mut found = domains(&joined);
    let host = hostname(&if blog.contains("://") { blog } else { format!("https://{blog}") });
    if !host.is_empty() {
        found.insert(host.strip_prefix("www.").unwrap_or(&host).to_owned());
    }
    found
}

pub(super) fn is_spam(post: &Post, profile: &Value, context: &str, at: Timestamp) -> bool {
    if post.kind == "pr" || !post.is_user || !NO_STANDING.contains(&post.association.as_str()) {
        return false;
    }
    let known = domains(context);
    let outside: BTreeSet<String> = domains(&post.body).difference(&known).cloned().collect();
    if outside.is_empty() {
        return false;
    }
    let young = profile.get("created_at").and_then(Value::as_str).and_then(|created| Timestamp::parse(created).ok()).is_some_and(|created| at.unix_seconds() - created.unix_seconds() < YOUNG);
    young || !outside.is_disjoint(&profile_domains(profile))
}

pub(super) fn summary(post: &Post) -> String {
    let flat = post.body.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = flat.chars().take(140).collect();
    let text = if flat.chars().count() > 140 { format!("{cut}…") } else { cut };
    text.replace('"', "'")
}

fn pages(gh: &dyn Gh, endpoint: &str, key: Option<&str>) -> Result<Vec<Value>, Failure> {
    let reply = gh.api(&["--paginate".into(), "--slurp".into(), endpoint.to_owned()], false)?;
    let mut rows = Vec::new();
    for page in reply.as_ref().and_then(Value::as_array).into_iter().flatten() {
        let inner = match key {
            Some(key) => page.get(key),
            None => Some(page),
        };
        rows.extend(inner.and_then(Value::as_array).cloned().unwrap_or_default());
    }
    Ok(rows)
}

struct Fields<'a> {
    actor: &'a Value,
    body: &'a str,
    association: &'a Value,
    created: &'a str,
    url: &'a Value,
}

fn post(kind: &'static str, repo: &str, number: i64, node: &Value, fields: Fields) -> Option<Post> {
    let Fields { actor, body, association, created, url } = fields;
    let id = match node.get("node_id").or_else(|| node.get("id"))? {
        Value::String(id) => id.clone(),
        other => other.to_string(),
    };
    let account_type = actor.get("type").or_else(|| actor.get("__typename")).and_then(Value::as_str);
    Some(Post {
        kind,
        repo: repo.to_owned(),
        number,
        id,
        actor: actor.get("login")?.as_str()?.to_owned(),
        is_user: account_type == Some("User"),
        body: body.to_owned(),
        association: association.as_str().unwrap_or("").to_owned(),
        created: created.to_owned(),
        url: url.as_str().unwrap_or("").to_owned(),
    })
}

fn last_number(url: &Value) -> i64 {
    url.as_str().and_then(|url| url.rsplit('/').next()).and_then(|tail| tail.parse().ok()).unwrap_or(0)
}

struct Issues {
    posts: Vec<Post>,
    repos: BTreeSet<String>,
    prs: BTreeSet<(String, i64)>,
}

fn issue_posts(config: &Config, gh: &dyn Gh, since: &str) -> Result<Issues, Failure> {
    let mut found = Issues { posts: Vec::new(), repos: BTreeSet::new(), prs: BTreeSet::new() };
    let search = format!("user:{} updated:>={since}", config.owner);
    let query = utf8_percent_encode(&search, QUOTE);
    for hit in pages(gh, &format!("search/issues?q={query}&per_page=100"), Some("items"))? {
        let repo = text(&hit, "repository_url").rsplit('/').next().unwrap_or("").to_owned();
        let number = hit["number"].as_i64().unwrap_or(0);
        let is_pr = hit.get("pull_request").is_some();
        found.repos.insert(repo.clone());
        if is_pr {
            found.prs.insert((repo.clone(), number));
        }
        if text(&hit, "created_at").as_str() >= since {
            let body = format!("{}\n{}", text(&hit, "title"), text(&hit, "body"));
            found.posts.extend(post(if is_pr { "pr" } else { "issue" }, &repo, number, &hit, Fields { actor: &hit["user"], body: &body, association: &hit["author_association"], created: &text(&hit, "created_at"), url: &hit["html_url"] }));
        }
    }
    Ok(found)
}

fn comment_posts(config: &Config, gh: &dyn Gh, since: &str, repos: &BTreeSet<String>) -> Result<Vec<Post>, Failure> {
    let mut found = Vec::new();
    for repo in repos {
        for (endpoint, number_key, kind) in [("issues/comments", "issue_url", "comment"), ("pulls/comments", "pull_request_url", "review comment")] {
            for item in pages(gh, &format!("repos/{}/{repo}/{endpoint}?since={since}&per_page=100", config.owner), None)? {
                if text(&item, "created_at").as_str() >= since {
                    found.extend(post(kind, repo, last_number(&item[number_key]), &item, Fields { actor: &item["user"], body: &text(&item, "body"), association: &item["author_association"], created: &text(&item, "created_at"), url: &item["html_url"] }));
                }
            }
        }
    }
    Ok(found)
}

fn review_posts(config: &Config, gh: &dyn Gh, since: &str, prs: &BTreeSet<(String, i64)>) -> Result<Vec<Post>, Failure> {
    let mut found = Vec::new();
    for (repo, number) in prs {
        for review in pages(gh, &format!("repos/{}/{repo}/pulls/{number}/reviews?per_page=100", config.owner), None)? {
            if text(&review, "submitted_at").as_str() >= since && review.get("user").is_some_and(|user| !user.is_null()) {
                found.extend(post("review", repo, *number, &review, Fields { actor: &review["user"], body: &text(&review, "body"), association: &review["author_association"], created: &text(&review, "submitted_at"), url: &review["html_url"] }));
            }
        }
    }
    Ok(found)
}

fn discussion_thread(discussion: &Value, since: &str) -> Vec<Post> {
    let repo = discussion.pointer("/repository/name").and_then(Value::as_str).unwrap_or("");
    let number = discussion["number"].as_i64().unwrap_or(0);
    let mut posts = vec![("discussion", discussion, format!("{}\n{}", text(discussion, "title"), text(discussion, "body")))];
    for comment in discussion.pointer("/comments/nodes").and_then(Value::as_array).into_iter().flatten() {
        posts.push(("discussion comment", comment, text(comment, "body")));
        for reply in comment.pointer("/replies/nodes").and_then(Value::as_array).into_iter().flatten() {
            posts.push(("discussion reply", reply, text(reply, "body")));
        }
    }
    let mut found = Vec::new();
    for (kind, node, body) in posts {
        let kind: &'static str = match kind {
            "discussion" => "discussion",
            "discussion comment" => "discussion comment",
            _ => "discussion reply",
        };
        if text(node, "createdAt").as_str() >= since && node.get("author").is_some_and(|author| !author.is_null()) {
            found.extend(post(kind, repo, number, node, Fields { actor: &node["author"], body: &body, association: &node["authorAssociation"], created: &text(node, "createdAt"), url: &node["url"] }));
        }
    }
    found
}

fn discussion_posts(config: &Config, gh: &dyn Gh, since: &str) -> Result<Vec<Post>, Failure> {
    let owner = &config.owner;
    let data = gh.api(&["graphql".into(), "-f".into(), format!("query={DISCUSSIONS}"), "-f".into(), format!("q=user:{owner} updated:>={since}")], false)?;
    let nodes = data.as_ref().and_then(|data| data.pointer("/data/search/nodes")).and_then(Value::as_array).cloned().unwrap_or_default();
    Ok(nodes.iter().filter(|node| !node.is_null()).flat_map(|discussion| discussion_thread(discussion, since)).collect())
}

pub(super) fn scan(config: &Config, gh: &dyn Gh, since: &str) -> Result<Vec<Post>, Failure> {
    let issues = issue_posts(config, gh, since)?;
    let mut found = issues.posts;
    found.extend(comment_posts(config, gh, since, &issues.repos)?);
    found.extend(review_posts(config, gh, since, &issues.prs)?);
    found.extend(discussion_posts(config, gh, since)?);
    found.retain(|found| !config.own.contains(&found.actor));
    Ok(found)
}

fn thread_context(config: &Config, gh: &dyn Gh, post: &Post) -> Result<String, Failure> {
    if post.kind == "issue" || post.kind == "pr" || post.kind.starts_with("discussion") {
        return Ok(String::new());
    }
    let base = format!("repos/{}/{}/issues/{}", config.owner, post.repo, post.number);
    let issue = gh.api(std::slice::from_ref(&base), false)?.unwrap_or(Value::Null);
    let comments = pages(gh, &format!("{base}/comments?per_page=100"), None)?;
    let mut texts = vec![text(&issue, "title"), text(&issue, "body")];
    texts.extend(comments.iter().filter(|comment| comment.pointer("/user/login").and_then(Value::as_str) != Some(post.actor.as_str())).map(|comment| text(comment, "body")));
    Ok(texts.join("\n"))
}

pub(super) fn classify(config: &Config, gh: &dyn Gh, post: &Post, at: Timestamp, profiles: &mut BTreeMap<String, Value>) -> Result<Class, Failure> {
    if post.kind == "pr" || !NO_STANDING.contains(&post.association.as_str()) || domains(&post.body).is_empty() {
        return Ok(Class::Real);
    }
    if !profiles.contains_key(&post.actor) {
        let profile = gh.api(&[format!("users/{}", post.actor)], false)?.unwrap_or_else(|| Value::Object(Default::default()));
        profiles.insert(post.actor.clone(), profile);
    }
    let context = thread_context(config, gh, post)?;
    Ok(if is_spam(post, &profiles[&post.actor], &context, at) { Class::Spam } else { Class::Real })
}

fn logins(items: &[Value], when: &str, since: &str, user: &str) -> BTreeSet<String> {
    items
        .iter()
        .filter(|item| item.get(when).and_then(Value::as_str).unwrap_or("") > since)
        .filter_map(|item| item.pointer(user).and_then(Value::as_str).map(str::to_owned))
        .collect()
}

pub(super) fn ours(config: &Config, gh: &dyn Gh, thread: &Value) -> Result<bool, Failure> {
    let kind = thread.pointer("/subject/type").and_then(Value::as_str).unwrap_or("");
    let url = thread.pointer("/subject/url").and_then(Value::as_str).unwrap_or("");
    if kind == "CheckSuite" || kind == "WorkflowRun" {
        return Ok(true);
    }
    if kind == "Release" && !url.is_empty() {
        let release = gh.api(&[url.to_owned()], true)?.unwrap_or(Value::Null);
        return Ok(release.pointer("/author/login").and_then(Value::as_str).is_some_and(|login| config.own.contains(login)));
    }
    if (kind != "Issue" && kind != "PullRequest") || url.is_empty() {
        return Ok(false);
    }
    let since = thread.get("last_read_at").and_then(Value::as_str).unwrap_or("");
    let issue_url = url.replace("/pulls/", "/issues/");
    let Some(issue) = gh.api(std::slice::from_ref(&issue_url), true)? else { return Ok(false) };
    let mut actors = BTreeSet::new();
    if text(&issue, "created_at").as_str() > since {
        actors.extend(issue.pointer("/user/login").and_then(Value::as_str).map(str::to_owned));
    }
    if text(&issue, "closed_at").as_str() > since {
        actors.extend(issue.pointer("/closed_by/login").and_then(Value::as_str).map(str::to_owned));
    }
    let query = if since.is_empty() { "?per_page=100".to_owned() } else { format!("?since={since}&per_page=100") };
    actors.extend(logins(&pages(gh, &format!("{issue_url}/comments{query}"), None)?, "created_at", since, "/user/login"));
    if kind == "PullRequest" {
        actors.extend(logins(&pages(gh, &format!("{url}/reviews?per_page=100"), None)?, "submitted_at", since, "/user/login"));
        actors.extend(logins(&pages(gh, &format!("{url}/comments{query}"), None)?, "created_at", since, "/user/login"));
    }
    Ok(actors.is_subset(&config.own))
}
