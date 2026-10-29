use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Gc;
use super::evidence::{item_ref, named_issues, origin_repo};
use super::exec::{api, command, git, real, text, truthy, verified};
use crate::foundation::clock::Timestamp;

const RUN_STATUSES: [&str; 5] = ["queued", "in_progress", "waiting", "pending", "requested"];

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct Trees {
    pub(super) ok: bool,
    pub(super) branches: BTreeMap<String, String>,
}

pub(super) struct Hosts {
    pub(super) laptop: Trees,
    pub(super) nexus: Trees,
    pub(super) on_nexus: bool,
}

impl Hosts {
    pub(super) fn peer(&self) -> (&'static str, &Trees) {
        if self.on_nexus { ("Laptop", &self.laptop) } else { ("Nexus", &self.nexus) }
    }
}

pub(super) struct GithubFacts {
    prs: HashSet<String>,
    runs: Vec<Value>,
}

fn alive_status(session: &Value) -> bool {
    session.get("status").and_then(Value::as_str) != Some("exited")
}

fn js_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => "undefined".to_string(),
    }
}

pub(super) fn pid_alive(holder: &str) -> bool {
    holder.bytes().all(|b| b.is_ascii_digit())
        && holder.parse::<i32>().ok().filter(|pid| *pid > 0).and_then(rustix::process::Pid::from_raw).is_some_and(|pid| rustix::process::test_kill_process(pid).is_ok())
}

impl Gc {
    pub(super) fn fleet_sessions(&mut self) -> Result<&[Value], String> {
        if self.fleet.is_none() {
            let listed = command(&["north", "fleet", "list", "--json"]);
            if !listed.ok {
                return Err("Fleet sessions unavailable".into());
            }
            let sessions: Value = serde_json::from_str(&listed.out).map_err(|error| error.to_string())?;
            let Value::Array(sessions) = sessions else {
                return Err("Fleet session list malformed".into());
            };
            self.fleet = Some(sessions);
        }
        Ok(self.fleet.as_deref().unwrap_or_default())
    }

    pub(super) fn owner(&mut self, name: &str) -> Result<String, String> {
        let prefix = format!("/code/{name}/");
        let sessions = self.fleet_sessions()?;
        let holder = sessions.iter().find(|session| {
            session.get("cwd").and_then(Value::as_str).is_some_and(|cwd| real(Path::new(cwd)).to_string_lossy().contains(&prefix)) && alive_status(session)
        });
        let named = |key| holder.and_then(|session| session.get(key)).and_then(text).map(str::to_string);
        Ok(named("parent").or_else(|| named("name")).unwrap_or_else(|| "proxy".into()))
    }

    pub(super) fn session_in(&mut self, path: &Path) -> Result<bool, String> {
        let lane = real(path);
        Ok(self.fleet_sessions()?.iter().any(|session| {
            session.get("cwd").and_then(Value::as_str).is_some_and(|cwd| real(Path::new(cwd)).starts_with(&lane)) && alive_status(session)
        }))
    }

    pub(super) fn live_claims(&mut self) -> Result<HashSet<String>, String> {
        if let Some(claims) = &self.claims {
            return Ok(claims.clone());
        }
        let listed = command(&["threads", "list"]);
        if !listed.ok || !listed.out.starts_with("item") {
            return Err("NorthThread claims unavailable".into());
        }
        let sessions = self.fleet_sessions()?;
        let alive = |holder: &str| {
            sessions.iter().any(|s| {
                let conversation = js_text(s.get("conversation"));
                let provider = js_text(s.get("provider"));
                (s.get("conversation").and_then(Value::as_str) == Some(holder)
                    || s.get("name").and_then(Value::as_str) == Some(holder)
                    || format!("{provider}:{conversation}") == holder)
                    && alive_status(s)
            }) || pid_alive(holder)
        };
        let claims: HashSet<String> = listed
            .out
            .lines()
            .skip(1)
            .filter_map(|line| {
                let row: Vec<&str> = line.split_whitespace().collect();
                (row.len() >= 2 && alive(row[1])).then(|| item_ref(row[0]))
            })
            .collect();
        self.claims = Some(claims.clone());
        Ok(claims)
    }

    pub(super) fn pushed_refs(&mut self, repo: &str) -> Option<HashMap<String, i64>> {
        if let Some(memo) = self.pushes.get(repo) {
            return memo.clone();
        }
        let refs = push_activity(repo);
        self.pushes.insert(repo.to_string(), refs.clone());
        refs
    }

    fn github_facts(&mut self, repo: &str) -> Result<&GithubFacts, String> {
        if !self.github.contains_key(repo) {
            let prs = open_pull_heads(repo)?;
            let mut runs = Vec::new();
            for status in RUN_STATUSES {
                for page in pages(&api(&format!("repos/tompassarelli/{repo}/actions/runs?status={status}&per_page=100"))?)? {
                    let Some(Value::Array(found)) = page.get("workflow_runs") else {
                        return Err("Actions eligibility incomplete".into());
                    };
                    runs.extend(found.iter().cloned());
                }
            }
            self.github.insert(repo.to_string(), GithubFacts { prs, runs });
        }
        Ok(&self.github[repo])
    }

    pub(super) fn local_archive_guard(&mut self, main: &Path, name: &str, base: &str, branch: &str, tip: &str, path: Option<&Path>) -> String {
        self.archive_guard(main, name, base, branch, tip, path).unwrap_or_else(|_| "liveness unavailable".into())
    }

    fn archive_guard(&mut self, main: &Path, name: &str, base: &str, branch: &str, tip: &str, path: Option<&Path>) -> Result<String, String> {
        let claims = self.live_claims()?;
        if named_issues(main, name, base, tip)?.iter().any(|item| claims.contains(item)) {
            return Ok("live holder session".into());
        }
        if let Some(path) = path
            && self.session_in(path)?
        {
            return Ok("live session in lane".into());
        }
        if !branch.is_empty() && verified(main, &format!("refs/remotes/origin/{branch}")).is_some() {
            let (peer, trees) = self.hosts.peer();
            if !trees.ok {
                return Ok(format!("{peer} liveness unavailable"));
            }
            if let Some(why) = trees.branches.get(&format!("{name}:{branch}")).filter(|why| !why.is_empty()) {
                return Ok(format!("{peer} {why}"));
            }
        }
        let Some(repo) = origin_repo(main) else {
            return Ok(String::new());
        };
        let Some(pushes) = self.pushed_refs(&repo) else {
            return Ok("push activity unavailable".into());
        };
        if !branch.is_empty() && pushes.contains_key(&format!("refs/heads/{branch}")) {
            return Ok("branch pushed under 24h".into());
        }
        if let Some(why) = autoland_hold(&repo, tip)? {
            return Ok(why.into());
        }
        let facts = self.github_facts(&repo)?;
        if !branch.is_empty() && facts.prs.contains(branch) {
            return Ok("open PR".into());
        }
        let runs_here = |run: &Value| {
            (!branch.is_empty() && run.get("head_branch").and_then(Value::as_str) == Some(branch)) || run.get("head_sha").and_then(Value::as_str) == Some(tip)
        };
        Ok(if facts.runs.iter().any(runs_here) { "active Actions".into() } else { String::new() })
    }
}

pub(super) fn pages(value: &Value) -> Result<&[Value], String> {
    value.as_array().map(Vec::as_slice).ok_or_else(|| "GitHub pages malformed".to_string())
}

pub(super) fn open_pull_heads(repo: &str) -> Result<HashSet<String>, String> {
    let listed = api(&format!("repos/tompassarelli/{repo}/pulls?state=open&per_page=100"))?;
    let mut heads = HashSet::new();
    for page in pages(&listed)? {
        for pull in page.as_array().map_or(std::slice::from_ref(page), Vec::as_slice) {
            let head = pull.get("head").filter(|head| head.is_object()).ok_or("pull request head missing")?;
            heads.insert(js_text(head.get("ref")));
        }
    }
    Ok(heads)
}

pub(super) fn pending_autoland(status: Option<&Value>) -> bool {
    status
        .and_then(|status| status.get("contexts"))
        .and_then(Value::as_array)
        .is_some_and(|contexts| contexts.iter().any(|s| s.get("context").and_then(Value::as_str) == Some("autoland") && s.get("state").and_then(Value::as_str) == Some("PENDING")))
}

fn autoland_hold(repo: &str, tip: &str) -> Result<Option<&'static str>, String> {
    let query = format!(
        "{{repository(owner:\"tompassarelli\",name:{}){{object(oid:{}){{... on Commit{{oid status{{contexts{{context state}}}}}}}}}}}}",
        serde_json::to_string(repo).unwrap_or_default(),
        serde_json::to_string(tip).unwrap_or_default()
    );
    let response = command(&["gh", "api", "graphql", "-f", &format!("query={query}")]);
    if !response.ok {
        return Ok(Some("Autoland eligibility unavailable"));
    }
    let page: Value = serde_json::from_str(&response.out).map_err(|error| error.to_string())?;
    let repository = page.get("data").and_then(|data| data.get("repository"));
    if truthy(page.get("errors")) || !truthy(repository) || repository.and_then(|r| r.get("object")).is_none() {
        return Ok(Some("Autoland eligibility incomplete"));
    }
    let object = repository.and_then(|r| r.get("object")).filter(|object| truthy(Some(object)));
    if object.is_some_and(|object| !truthy(object.get("oid")) || object.get("status").is_none()) {
        return Ok(Some("Autoland status incomplete"));
    }
    Ok(pending_autoland(object.and_then(|object| object.get("status"))).then_some("pending autoland"))
}

fn push_activity(repo: &str) -> Option<HashMap<String, i64>> {
    let listed = api(&format!("repos/tompassarelli/{repo}/activity?time_period=day&per_page=100")).ok()?;
    let pages = listed.as_array().filter(|pages| !pages.is_empty() && pages.iter().all(Value::is_array))?;
    let rows: Vec<&Value> = pages.iter().filter_map(Value::as_array).flatten().collect();
    let mut refs = HashMap::new();
    let mut parsed = Vec::with_capacity(rows.len());
    for row in rows {
        let kind = row.get("activity_type").and_then(Value::as_str)?;
        let name = row.get("ref").and_then(Value::as_str)?;
        let at = Timestamp::parse_rfc3339(row.get("timestamp").and_then(Value::as_str)?)?.unix_millis();
        parsed.push((kind, name, at));
    }
    for (kind, name, at) in parsed {
        if matches!(kind, "push" | "force_push" | "branch_creation") {
            let newest = refs.entry(name.to_string()).or_insert(0);
            *newest = (*newest).max(at);
        }
    }
    Some(refs)
}

pub(super) fn tree_is_live<'a>(trees: &'a Trees, key: &str) -> Option<&'a str> {
    trees.branches.get(key).map(String::as_str).filter(|why| !why.is_empty())
}

pub(super) fn holds_git_dir(path: &Path, common: &Path) -> bool {
    common.parent().is_some_and(|parent| real(path) == parent)
}

pub(super) fn worktree_admin(path: &Path) -> String {
    git(path, &["rev-parse", "--path-format=absolute", "--git-dir"]).out
}
