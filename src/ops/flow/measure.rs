//! One repository's flow row: what is queued, how fast lanes land, whether landing has stalled.
//! A source that cannot be read leaves its fields at `"error"` and later fields unread.
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use super::autoland::{self, Latency};
use super::goal::goal_finish;
use super::guards::{pin_builds, pins_verdict, reuse_verdict, wisp_row};
use super::host::{Host, Places, iso, read_json, seconds, write_atomic};
use super::jobs::landing_jobs;
use super::stall::{self, Facts, Job};
use super::throughput::{self, Closed, Commit, DeadLane, Inputs};

pub(crate) type Row = Map<String, Value>;

const CACHE_SECONDS: f64 = 300.0;
const DAY: f64 = 86_400.0;
const WISP_REPO: &str = "smashcraft";
const THROUGHPUT: [&str; 6] = ["closed_per_hour", "needed_per_hour", "land_to_close_minutes", "land_ratio_3h", "failed_unowned", "oldest_lane_wait_minutes"];

static GITHUB: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:https://github\.com/|git@github\.com:|ssh://git@github\.com/)([\w.-]+/[\w.-]+?)(?:\.git)?/?$").unwrap());
static CLAIM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([\w.-]+)#(\d+)$").unwrap());
static MINUTES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)m$").unwrap());

pub(crate) struct Env<'a> {
    pub(crate) places: &'a Places,
    pub(crate) host: &'a dyn Host,
    pub(crate) targets: BTreeMap<String, f64>,
    claims: RefCell<Option<BTreeMap<String, BTreeMap<u64, f64>>>>,
}

impl<'a> Env<'a> {
    pub(crate) fn new(places: &'a Places, host: &'a dyn Host, targets: BTreeMap<String, f64>) -> Self {
        Self { places, host, targets, claims: RefCell::default() }
    }

    /// Claim start times by repository and issue, from `threads list`, read once per run.
    fn claims(&self, repo: &str, now: f64) -> BTreeMap<u64, f64> {
        self.claims.borrow_mut().get_or_insert_with(|| claim_times(self.host, now)).get(repo).cloned().unwrap_or_default()
    }
}

fn claim_times(host: &dyn Host, now: f64) -> BTreeMap<String, BTreeMap<u64, f64>> {
    let mut claims: BTreeMap<String, BTreeMap<u64, f64>> = BTreeMap::new();
    for line in host.run(&["threads", "list"], None, None).unwrap_or_default().lines().skip(1) {
        let columns: Vec<&str> = line.split_whitespace().collect();
        let (Some(item), Some(clock)) = (columns.first().and_then(|column| CLAIM.captures(column)), columns.get(2).and_then(|column| MINUTES.captures(column))) else { continue };
        if let (Ok(number), Ok(minutes)) = (item[2].parse::<u64>(), clock[1].parse::<f64>()) {
            claims.entry(item[1].to_owned()).or_default().insert(number, now - minutes * 60.0);
        }
    }
    claims
}

pub(crate) fn github_repo(url: &str) -> Option<String> {
    GITHUB.captures(url.trim()).map(|found| found[1].to_owned())
}

/// Read `orchestration.toml`'s project targets as epoch seconds.
pub(crate) fn project_targets(path: &Path) -> BTreeMap<String, f64> {
    let Ok(text) = std::fs::read_to_string(path) else { return BTreeMap::new() };
    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else { return BTreeMap::new() };
    let Some(projects) = document.get("projects").and_then(toml_edit::Item::as_table_like) else { return BTreeMap::new() };
    projects
        .iter()
        .filter_map(|(name, project)| {
            let target = project.get("target")?;
            let text = target.as_datetime().map_or_else(|| target.as_str().map(str::to_owned), |at| Some(at.to_string()))?;
            Some((name.to_owned(), seconds(&text)?))
        })
        .collect()
}

struct Source<'a> {
    env: &'a Env<'a>,
    repo: &'a str,
    path: PathBuf,
    slug: String,
    now: f64,
    autoland: bool,
}

impl Source<'_> {
    fn git(&self, args: &[&str]) -> Option<String> {
        let mut command = vec!["git"];
        command.extend(args);
        self.env.host.run(&command, Some(&self.path), None)
    }
}

fn error() -> Value {
    Value::from("error")
}

fn initial(env: &Env, repo: &str, now: f64) -> Row {
    let target = env.targets.get(repo).copied();
    let mut row = Row::new();
    for (key, value) in [
        ("repo", json!(repo)),
        ("epoch", json!(now)),
        ("target", json!(target)),
        ("pending", error()),
        ("landed_per_hour", error()),
        ("push_to_land_minutes", error()),
        ("push_to_land_p90_minutes", error()),
        ("main_age_minutes", error()),
        ("oldest_job", error()),
        ("stall", error()),
        ("wisp", Value::Null),
        ("reuse", json!(reuse_verdict(None))),
        ("pins", json!(pins_verdict(None))),
    ] {
        row.insert(key.into(), value);
    }
    for field in THROUGHPUT {
        row.insert(field.into(), error());
    }
    row.insert("goal_finish".into(), json!(goal_finish(env.places, repo, target)));
    row
}

pub(crate) fn measure(env: &Env, repo: &str, now: f64) -> Row {
    let mut row = initial(env, repo, now);
    let _ = fill(env, repo, now, &mut row);
    row
}

fn fill(env: &Env, repo: &str, now: f64, row: &mut Row) -> Option<()> {
    let path = env.places.checkout(repo);
    let slug = github_repo(&env.host.run(&["git", "remote", "get-url", "origin"], Some(&path), None)?)?;
    if repo == WISP_REPO {
        row.insert("wisp".into(), json!(wisp_row(env.host, &env.places.home, &slug, now)));
    }
    let mut source = Source { env, repo, path, slug, now, autoland: false };
    source.git(&["fetch", "--quiet", "--prune", "--no-write-fetch-head", "origin", "+refs/heads/main:refs/remotes/origin/main", "+refs/heads/claude/land-*:refs/remotes/origin/claude/land-*"])?;
    let queue = source.git(&["config", "--blob", "origin/main:.safe-push", "--get", "landing.queue"]);
    source.autoland = queue.is_some_and(|queue| queue.trim() == "autoland");
    row.insert("pins".into(), json!(pins_verdict(pin_builds(env.host, &source.path).as_deref())));
    let landing = source.landing(row);
    let tips = source.lane_tips()?;
    let (unlanded, cleanup) = source.live_lanes(&tips, source.unlanded(&tips));
    row.insert("cleanup".into(), json!(cleanup));
    let states = if source.autoland { autoland::autoland_states(env.host, &source.slug, &unlanded) } else { BTreeMap::new() };
    let (lanes, dead_count, dead_oldest) = autoland::lane_counts(&unlanded, &states, now);
    let mut pending = lanes as u64;
    row.insert("dead".into(), json!(dead_count));
    row.insert("dead_oldest_minutes".into(), json!(dead_oldest));
    let dead: BTreeSet<&String> = unlanded.iter().filter(|sha| autoland::failed_since(&states, sha).is_some()).collect();
    let mut pending_since: Vec<f64> = unlanded.iter().filter(|sha| !dead.contains(sha)).map(|sha| tips[sha].0).collect();
    source.add_throughput(row, landing.as_ref(), &tips, &states, &dead, &pending_since);
    if source.autoland {
        let (count, since) = source.queued_runs()?;
        pending += count;
        pending_since.extend(since);
    }
    row.insert("pending".into(), json!(pending));
    source.add_stall(row, landing.as_ref()?, pending, &pending_since)
}

struct Landing {
    commits: Vec<(String, i64)>,
    pushes: Option<Vec<f64>>,
    landed: Option<Vec<f64>>,
}

impl Source<'_> {
    fn landing(&self, row: &mut Row) -> Option<Landing> {
        let log = self.git(&["log", "--first-parent", &format!("--since-as-filter={}", (self.now - DAY) as i64), "--format=%H %ct", "origin/main"])?;
        let commits: Vec<(String, i64)> = log.lines().filter_map(|line| line.split_once(' ')).filter_map(|(sha, at)| Some((sha.to_owned(), at.parse().ok()?))).collect();
        let recent = commits.iter().filter(|(_, at)| *at as f64 >= self.now - 10_800.0).count();
        row.insert("landed_per_hour".into(), json!(recent as f64 / 3.0));
        let (median, p90, pushes, landed) = if self.autoland {
            let Latency { median, p90, reuse, pushes, landed } = autoland::autoland_latency(self.env.host, &self.slug, self.now);
            row.insert("reuse".into(), json!(reuse_verdict(reuse.as_deref())));
            (median, p90, pushes, landed)
        } else {
            let (median, p90) = autoland::local_latency(self.env.host, self.env.places, &self.path, &self.slug, &commits);
            (median, p90, Some(autoland::local_push_times(self.env.places, &self.slug)), None)
        };
        row.insert("push_to_land_minutes".into(), median);
        row.insert("push_to_land_p90_minutes".into(), p90);
        Some(Landing { commits, pushes, landed })
    }

    fn lane_tips(&self) -> Option<BTreeMap<String, (f64, String)>> {
        let branches = self.git(&["for-each-ref", "--format=%(objectname) %(committerdate:unix) %(refname:lstrip=3)", "refs/remotes/origin/claude/land-*"])?;
        Some(
            branches
                .lines()
                .filter_map(|line| {
                    let mut parts = line.splitn(3, ' ');
                    Some((parts.next()?.to_owned(), (parts.next()?.parse().ok()?, parts.next()?.to_owned())))
                })
                .collect(),
        )
    }

    fn unlanded(&self, tips: &BTreeMap<String, (f64, String)>) -> Vec<String> {
        let open = self.git(&["for-each-ref", "--no-merged=origin/main", "--format=%(objectname)", "refs/remotes/origin/claude/land-*"]);
        let open: Option<BTreeSet<&str>> = open.as_deref().map(|listed| listed.split_whitespace().collect());
        tips.keys()
            .filter(|sha| match &open {
                Some(open) => open.contains(sha.as_str()),
                None => self.git(&["merge-base", "--is-ancestor", sha, "origin/main"]).is_none(),
            })
            .cloned()
            .collect()
    }

    /// The unlanded lanes still in play, and the branches to clean up: pushed over a day ago, or whose
    /// merge into main adds nothing because the change landed in another form.
    fn live_lanes(&self, tips: &BTreeMap<String, (f64, String)>, unlanded: Vec<String>) -> (Vec<String>, Vec<String>) {
        let main_tree = self.git(&["rev-parse", "origin/main^{tree}"]).map(|tree| tree.trim().to_owned());
        let (mut live, mut cleanup) = (Vec::new(), Vec::new());
        for sha in unlanded {
            let (pushed, branch) = &tips[&sha];
            let merged = self.git(&["merge-tree", "--write-tree", "origin/main", &sha]);
            let landed = matches!((&merged, &main_tree), (Some(merged), Some(tree)) if merged.lines().next() == Some(tree.as_str()));
            if self.now - pushed > DAY || landed {
                cleanup.push(branch.clone());
            } else {
                live.push(sha);
            }
        }
        (live, cleanup)
    }

    fn queued_runs(&self) -> Option<(u64, Vec<f64>)> {
        let mut count = 0;
        let mut since = Vec::new();
        for status in ["queued", "in_progress"] {
            let listed = self.env.host.json(&["gh", "run", "list", "--repo", &self.slug, "--workflow", "autoland.yml", "--status", status, "--limit", "1000", "--json", "displayTitle,createdAt"])?;
            for run in listed.as_array()?.iter().filter(|run| run["displayTitle"].as_str().is_some_and(|title| title.starts_with("Autoland test "))) {
                count += 1;
                since.push(seconds(run["createdAt"].as_str()?)?);
            }
        }
        Some((count, since))
    }
}

impl Source<'_> {
    fn main_log(&self) -> Option<Vec<Commit>> {
        let history = self.git(&["log", "--first-parent", &format!("--since-as-filter={}", (self.now - 7.0 * DAY) as i64), "--format=%H%x1f%ct%x1f%B%x1e", "origin/main"])?;
        Some(
            history
                .split('\x1e')
                .filter(|entry| !entry.trim().is_empty())
                .filter_map(|entry| {
                    let mut parts = entry.trim_matches('\n').splitn(3, '\x1f');
                    Some(Commit { sha: parts.next()?.to_owned(), at: parts.next()?.parse().ok()?, message: parts.next()?.to_owned() })
                })
                .collect(),
        )
    }

    fn dead_lanes(&self, tips: &BTreeMap<String, (f64, String)>, states: &BTreeMap<String, Value>, dead: &BTreeSet<&String>) -> Vec<DeadLane> {
        dead.iter()
            .filter_map(|sha| {
                let named = self.git(&["log", "--format=%B", &format!("origin/main..{sha}")]).unwrap_or_default();
                let mut issues = throughput::issue_refs(&named, self.repo);
                issues.extend(throughput::branch_numbers(&tips[*sha].1));
                Some(DeadLane { failed_at: autoland::failed_since(states, sha)?, issues })
            })
            .collect()
    }

    fn add_throughput(&self, row: &mut Row, landing: Option<&Landing>, tips: &BTreeMap<String, (f64, String)>, states: &BTreeMap<String, Value>, dead: &BTreeSet<&String>, pending_since: &[f64]) {
        let (open_count, closed) = issue_activity(self.env.host, &self.slug, self.now);
        let (Some(landing), Some(closed), Some(main_log)) = (landing, closed, self.main_log()) else { return };
        let Some(pushes) = &landing.pushes else { return };
        let landings = landing.landed.clone().unwrap_or_else(|| landing.commits.iter().map(|(_, at)| *at as f64).collect());
        let inputs = Inputs {
            repo: self.repo,
            now: self.now,
            open_count,
            closed: &closed,
            main_log: &main_log,
            landings: &landings,
            pushes,
            target: self.env.targets.get(self.repo).copied(),
            dead_lanes: &self.dead_lanes(tips, states, dead),
            claims: &self.env.claims(self.repo, self.now),
            pending_since,
        };
        if let Ok(Value::Object(fields)) = serde_json::to_value(throughput::throughput(&inputs)) {
            row.extend(fields);
        }
    }

    fn add_stall(&self, row: &mut Row, landing: &Landing, pending: u64, pending_since: &[f64]) -> Option<()> {
        let tip = self.git(&["log", "-1", "--format=%H %ct", "origin/main"]).filter(|tip| !tip.trim().is_empty())?;
        let (sha, at) = tip.split_once(' ')?;
        let at: i64 = at.trim().parse().ok()?;
        row.insert("main_sha".into(), json!(sha));
        row.insert("main_age_minutes".into(), json!((self.now - at as f64).max(0.0) / 60.0));
        let (jobs, medians) = landing_jobs(self.env.places, self.env.host, &self.path, &self.slug, self.now)?;
        row.insert("oldest_job".into(), oldest_job(&jobs, self.now));
        let landings: Vec<f64> = landing.commits.iter().map(|(_, at)| *at as f64).collect();
        let facts = Facts { now: self.now, main_at: at as f64, landings: &landings, pending, jobs: &jobs, medians: &medians, pending_since: pending_since.iter().copied().min_by(f64::total_cmp) };
        row.insert("stall".into(), json!(stall::verdict(&facts)));
        Some(())
    }
}

fn oldest_job(jobs: &[Job], now: f64) -> Value {
    match jobs.iter().min_by(|a, b| a.started_at.total_cmp(&b.started_at)) {
        None => Value::Null,
        Some(job) => {
            let mut value = json!(job);
            value["age_minutes"] = json!((now - job.started_at).max(0.0) / 60.0);
            value
        }
    }
}

fn issue_activity(host: &dyn Host, slug: &str, now: f64) -> (Option<u64>, Option<Vec<Closed>>) {
    let (owner, name) = slug.split_once('/').unwrap_or((slug, ""));
    let search = format!("repo:{slug} is:issue is:closed closed:>={}", iso(now - DAY));
    let query = format!(
        "{{ repository(owner:{owner:?}, name:{name:?}) {{ issues(states: OPEN) {{ totalCount }} }} search(query:{search:?}, type: ISSUE, first: 100) {{ nodes {{ ... on Issue {{ number closedAt comments(last: 20) {{ nodes {{ body }} }} }} }} }} }}"
    );
    let read = |response: &Value| -> Option<(u64, Vec<Closed>)> {
        let data = &response["data"];
        let closed = data["search"]["nodes"]
            .as_array()?
            .iter()
            .filter(|item| !item.is_null())
            .map(|item| {
                let comments = item["comments"]["nodes"].as_array()?.iter().map(|comment| Some(comment["body"].as_str()?.to_owned())).collect::<Option<Vec<_>>>()?;
                throughput::closed_from(item["number"].as_u64()?, item["closedAt"].as_str()?, comments)
            })
            .collect::<Option<Vec<_>>>()?;
        Some((data["repository"]["issues"]["totalCount"].as_u64()?, closed))
    };
    match host.graphql(&query).as_ref().and_then(read) {
        Some((open, closed)) => (Some(open), Some(closed)),
        None => (None, None),
    }
}

fn cached(places: &Places, repo: &str, now: f64, fields: &[&str]) -> Option<Value> {
    let row = read_json(&places.flow().join(format!("{repo}.json")))?;
    let age = now - row["epoch"].as_f64()?;
    (row["repo"] == repo && fields.iter().all(|field| row.get(field).is_some()) && (0.0..CACHE_SECONDS).contains(&age)).then_some(row)
}

fn main_moved(env: &Env, repo: &str, row: &Value) -> bool {
    let tip = env.host.run(&["git", "rev-parse", "origin/main"], Some(&env.places.checkout(repo)), None);
    tip.is_some_and(|tip| row.get("main_sha").is_some_and(|sha| sha.as_str() != Some(tip.trim())))
}

/// The cached row while it is under five minutes old, main has not moved and the goal target is
/// unchanged; otherwise a fresh measurement, cached.
pub(crate) fn get(env: &Env, repo: &str, now: f64, fields: &[&str]) -> Value {
    let target = env.targets.get(repo).copied();
    if let Some(row) = cached(env.places, repo, now, fields)
        && row["target"].as_f64() == target
        && !main_moved(env, repo, &row)
    {
        return row;
    }
    let row = Value::Object(measure(env, repo, now));
    let _ = write_atomic(&env.places.flow().join(format!("{repo}.json")), &format!("{row}\n"));
    row
}
