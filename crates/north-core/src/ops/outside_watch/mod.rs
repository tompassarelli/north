mod scan;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;

const USAGE: &str = "usage: north outside-watch run [--dry-run] [--days N] | status";
const OVERLAP: i64 = 15 * 60;
const DAY: i64 = 86_400;
const MINIMIZE: &str = "mutation($id: ID!) { minimizeComment(input: {subjectId: $id, classifier: SPAM}) { minimizedComment { isMinimized } } }";

pub(super) type Failure = String;

pub(super) trait Gh {
    fn api(&self, args: &[String], ok_missing: bool) -> Result<Option<Value>, Failure>;
}

struct Cli;

impl Gh for Cli {
    fn api(&self, args: &[String], ok_missing: bool) -> Result<Option<Value>, Failure> {
        let output = Command::new("gh").arg("api").args(args).output().map_err(|error| format!("outside-watch: gh: {error}"))?;
        if !output.status.success() {
            if ok_missing {
                return Ok(None);
            }
            let reason: String = String::from_utf8_lossy(&output.stderr).trim().chars().take(300).collect();
            return Err(format!("outside-watch: gh api {} failed: {reason}", args.iter().take(2).cloned().collect::<Vec<_>>().join(" ")));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        if text.trim().is_empty() {
            return Ok(None);
        }
        serde_json::from_str(&text).map(Some).map_err(|error| format!("outside-watch: gh api returned invalid JSON: {error}"))
    }
}

pub(super) struct Config {
    owner: String,
    own: BTreeSet<String>,
    state_dir: PathBuf,
    inbox: PathBuf,
    now: Timestamp,
}

impl Config {
    fn from_env() -> Result<Self, Failure> {
        let var = |name: &str| env::var(name).ok().filter(|value| !value.is_empty());
        let home = PathBuf::from(env::var("HOME").unwrap_or_default());
        let now = match var("OUTSIDE_WATCH_NOW") {
            Some(text) => Timestamp::parse(&text).map_err(|error| format!("outside-watch: OUTSIDE_WATCH_NOW: {error}"))?,
            None => Timestamp::now(),
        };
        Ok(Self {
            owner: env::var("OUTSIDE_WATCH_OWNER").unwrap_or_else(|_| "tompassarelli".to_owned()),
            own: env::var("OUTSIDE_WATCH_OWN")
                .unwrap_or_else(|_| "tompassarelli,github-actions[bot],dependabot[bot]".to_owned())
                .split(',')
                .map(str::to_owned)
                .collect(),
            state_dir: var("OUTSIDE_WATCH_STATE").map_or_else(|| home.join(".local/state/outside-watch"), PathBuf::from),
            inbox: var("OUTSIDE_WATCH_INBOX").map_or_else(|| home.join(".local/state/agents/handoffs/proxy-inbox.md"), PathBuf::from),
            now,
        })
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let outcome = match arguments {
        [command] if command == "status" => Config::from_env().and_then(|config| status(&config, &mut std::io::stdout().lock())),
        [command, rest @ ..] if command == "run" => match options(rest) {
            Some((dry_run, days)) => Config::from_env().and_then(|config| check(&config, &Cli, dry_run, days, &mut std::io::stdout().lock())),
            None => return usage(),
        },
        _ => return usage(),
    };
    match outcome {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}

fn options(rest: &[String]) -> Option<(bool, Option<f64>)> {
    let (mut dry_run, mut days) = (false, None);
    let mut arguments = rest.iter();
    while let Some(flag) = arguments.next() {
        match flag.as_str() {
            "--dry-run" => dry_run = true,
            "--days" => days = Some(arguments.next()?.parse().ok()?),
            other => days = Some(other.strip_prefix("--days=")?.parse().ok()?),
        }
    }
    Some((dry_run, days.filter(|value: &f64| value.is_finite() && *value != 0.0)))
}

fn load_state(config: &Config) -> Value {
    fs::read_to_string(config.state_dir.join("state.json")).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_else(|| json!({}))
}

fn log(config: &Config, line: &str) -> Result<(), Failure> {
    let io = |error: std::io::Error| format!("outside-watch: {}: {error}", config.state_dir.display());
    fs::create_dir_all(&config.state_dir).map_err(io)?;
    let mut file = OpenOptions::new().create(true).append(true).open(config.state_dir.join("log")).map_err(io)?;
    writeln!(file, "{} {line}", config.now.iso()).map_err(io)
}

struct Tally {
    spam: usize,
    external: usize,
    lines: Vec<String>,
}

struct Run<'a> {
    config: &'a Config,
    gh: &'a dyn Gh,
    dry_run: bool,
}

fn flush(error: std::io::Error) -> Failure {
    format!("outside-watch: {error}")
}

fn since_of(config: &Config, state: &Value, days: Option<f64>) -> String {
    let at = config.now;
    match (days, state.get("last_run").and_then(Value::as_str).and_then(|text| Timestamp::parse(text).ok())) {
        (Some(days), _) => Timestamp::from_unix_seconds_f64(at.unix_seconds_f64() - days * DAY as f64).iso(),
        (None, Some(last)) => Timestamp::from_unix_seconds(last.unix_seconds() - OVERLAP).iso(),
        (None, None) => Timestamp::from_unix_seconds(at.unix_seconds() - OVERLAP).iso(),
    }
}

fn seen_of(state: &Value) -> BTreeMap<String, String> {
    state
        .get("seen")
        .and_then(Value::as_object)
        .map(|map| map.iter().filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned()))).collect())
        .unwrap_or_default()
}

impl Run<'_> {
    fn spam(&self, post: &scan::Post, place: &str, out: &mut dyn Write, tally: &mut Tally) -> Result<(), Failure> {
        if self.dry_run {
            writeln!(out, "SPAM {place}: {} \"{}\" {}", post.kind, scan::summary(post), post.url).map_err(flush)?;
            return Ok(());
        }
        let minimizable = ["comment", "review comment", "discussion comment", "discussion reply"].contains(&post.kind);
        if minimizable {
            self.gh.api(&["graphql".into(), "-f".into(), format!("query={MINIMIZE}"), "-f".into(), format!("id={}", post.id)], false)?;
        }
        if self.gh.api(&[format!("user/blocks/{}", post.actor)], true)?.is_none() {
            self.gh.api(&["-X".into(), "PUT".into(), format!("user/blocks/{}", post.actor)], false)?;
        }
        let done = if minimizable { "minimized and blocked" } else { "blocked, post left up" };
        tally.lines.push(format!("OPS SPAM {place}: {done}: {} \"{}\" {}", post.kind, scan::summary(post), post.url));
        Ok(())
    }

    fn external(&self, post: &scan::Post, place: &str, out: &mut dyn Write, tally: &mut Tally) -> Result<(), Failure> {
        let line = format!("OPS EXTERNAL {place}: {} \"{}\" {}", post.kind, scan::summary(post), post.url);
        if self.dry_run {
            writeln!(out, "{}", &line["OPS ".len()..]).map_err(flush)?;
        } else {
            tally.lines.push(line);
        }
        Ok(())
    }

    fn posts(&self, since: &str, seen: &mut BTreeMap<String, String>, out: &mut dyn Write) -> Result<Tally, Failure> {
        let mut profiles = BTreeMap::new();
        let mut tally = Tally { spam: 0, external: 0, lines: Vec::new() };
        for post in scan::scan(self.config, self.gh, since)? {
            if seen.contains_key(&post.id) && !self.dry_run {
                continue;
            }
            seen.insert(post.id.clone(), post.created.clone());
            let place = format!("{}#{} {}", post.repo, post.number, post.actor);
            if scan::classify(self.config, self.gh, &post, self.config.now, &mut profiles)? == scan::Class::Spam {
                tally.spam += 1;
                self.spam(&post, &place, out, &mut tally)?;
            } else {
                tally.external += 1;
                self.external(&post, &place, out, &mut tally)?;
            }
        }
        Ok(tally)
    }

    fn notifications(&self) -> Result<usize, Failure> {
        let mut marked = 0;
        for page in self.gh.api(&["--paginate".into(), "--slurp".into(), "notifications?per_page=50".into()], false)?.as_ref().and_then(Value::as_array).into_iter().flatten() {
            for thread in page.as_array().into_iter().flatten() {
                if scan::ours(self.config, self.gh, thread)? {
                    marked += 1;
                    if !self.dry_run {
                        let id = thread["id"].as_str().unwrap_or_default();
                        self.gh.api(&["-X".into(), "PATCH".into(), format!("notifications/threads/{id}")], false)?;
                    }
                }
            }
        }
        Ok(marked)
    }

    fn save(&self, tally: &Tally, result: &str, since: &str, seen: &mut BTreeMap<String, String>) -> Result<(), Failure> {
        let config = self.config;
        if !tally.lines.is_empty() {
            let io = |error: std::io::Error| format!("outside-watch: {}: {error}", config.inbox.display());
            let mut inbox = OpenOptions::new().create(true).append(true).open(&config.inbox).map_err(io)?;
            inbox.write_all(tally.lines.iter().map(|line| format!("{line}\n")).collect::<String>().as_bytes()).map_err(io)?;
        }
        for line in &tally.lines {
            log(config, line)?;
        }
        log(config, &format!("clean run {result}"))?;
        let keep_from = Timestamp::from_unix_seconds(Timestamp::parse(since).map_err(|error| error.to_string())?.unix_seconds() - DAY).iso();
        seen.retain(|_, created| created.as_str() >= keep_from.as_str());
        let saved = json!({"last_run": config.now.iso(), "seen": seen});
        let io = |error: std::io::Error| format!("outside-watch: {}: {error}", config.state_dir.display());
        fs::create_dir_all(&config.state_dir).map_err(io)?;
        fs::write(config.state_dir.join("state.json"), saved.to_string()).map_err(io)
    }
}

pub(super) fn check(config: &Config, gh: &dyn Gh, dry_run: bool, days: Option<f64>, out: &mut dyn Write) -> Result<(), Failure> {
    let run = Run { config, gh, dry_run };
    let state = load_state(config);
    let since = since_of(config, &state, days);
    let mut seen = seen_of(&state);
    let tally = run.posts(&since, &mut seen, out)?;
    let marked = run.notifications()?;
    let verb = if dry_run { "would mark" } else { "marked" };
    let result = format!("since={since} spam={} external={} {verb}-read={marked}", tally.spam, tally.external);
    writeln!(out, "{result}").map_err(flush)?;
    if dry_run {
        return Ok(());
    }
    run.save(&tally, &result, &since, &mut seen)
}

fn status(config: &Config, out: &mut dyn Write) -> Result<(), Failure> {
    let state = load_state(config);
    let flush = |error: std::io::Error| format!("outside-watch: {error}");
    writeln!(out, "last run: {}", state.get("last_run").and_then(Value::as_str).unwrap_or("never")).map_err(flush)?;
    if let Some(last) = fs::read_to_string(config.state_dir.join("log")).ok().and_then(|text| text.lines().last().map(str::to_owned)) {
        writeln!(out, "{last}").map_err(flush)?;
    }
    Ok(())
}
