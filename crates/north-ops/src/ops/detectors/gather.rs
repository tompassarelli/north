use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::Value;

use super::correction;
use super::disk;
use super::filing::Filer;
use super::plan::{self, Doc, IssueBox, Landed};
use super::refusal::{self, Refusal};
use super::signal::{self, Signal};
use super::slow_test;
use super::{Limits, default_number, default_text};
use crate::health::loops::{self, Declared, Origin};
use crate::ops::flow::{Host, Places};
use crate::platform::sqlite::{self, Access};

const LANE_RECENT_S: f64 = 24.0 * 3600.0;
const DISPATCH_RECENT_S: f64 = 2.0 * 3600.0;
const LANDED_HISTORY: &str = "--since=30 days ago";
const LOG_TAIL_LINES: usize = 400;
const QUEUE_LIVE: [&str; 3] = ["queued", "taken", "requeued"];
const PLAN_REPO: &str = "north";
const REFUSAL_DOC: &str = "landing-train";
const DISK_DOC: &str = "capacity-scheduler";
const OWNER_PREFIX: &str = "Owner: ";
const GROUNDING_LEDGER: &str = "north/grounding/ledger.jsonl";
const RUN_HISTORY: usize = 30;
const PROFILES_PER_PASS: usize = 1;
const PROFILE_GRACE: Duration = Duration::from_secs(30);
const ATTEMPTS: &str = "slow-test-attempts.json";
const DRY_RUN_PROFILE: &str = "its profile is taken only when the box is filed.";

pub struct Gatherer<'a> {
    places: &'a Places,
    host: &'a dyn Host,
    profiling: bool,
}

fn modified(path: &Path) -> Option<f64> {
    Some(fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64())
}

fn files(directory: &Path, extension: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.is_file() && path.extension().is_some_and(|found| found == extension)).collect();
    found.sort();
    found
}

fn subdirectories(directory: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
    found.sort();
    found
}

fn log_tail(path: &str) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(LOG_TAIL_LINES)..].join("\n")
}

fn log_path(text: &str) -> Option<&str> {
    text.split_whitespace().find(|word| word.starts_with('/') && word.ends_with(".log"))
}

fn with_log(text: &str) -> String {
    log_path(text).map_or_else(|| text.to_owned(), |path| format!("{text}\n{}", log_tail(path)))
}

fn owner_line(text: &str) -> Option<String> {
    text.lines().find_map(|line| line.strip_prefix(OWNER_PREFIX)).map(|owner| owner.trim().to_owned())
}

impl<'a> Gatherer<'a> {
    pub fn new(places: &'a Places, host: &'a dyn Host) -> Self {
        Self { places, host, profiling: false }
    }

    pub fn profiling(mut self, profiling: bool) -> Self {
        self.profiling = profiling;
        self
    }

    fn checkout(&self) -> PathBuf {
        self.places.checkout(PLAN_REPO)
    }

    fn git(&self, arguments: &[&str]) -> Option<String> {
        let checkout = self.checkout();
        let mut argv = vec!["git"];
        argv.extend_from_slice(arguments);
        self.host.run(&argv, Some(&checkout), None)
    }

    fn docs(&self) -> Vec<Doc> {
        files(&self.checkout().join("docs/rationale"), "md")
            .into_iter()
            .filter_map(|path| {
                let slug = path.file_stem()?.to_str()?.to_owned();
                let relative = format!("docs/rationale/{slug}.md");
                let added = self.git(&["log", "--diff-filter=A", "--format=%ct", "--", &relative])?;
                let added_at = added.lines().filter_map(|line| line.trim().parse::<f64>().ok()).reduce(f64::min)?;
                Some(Doc { slug, added_at, text: fs::read_to_string(&path).ok()? })
            })
            .collect()
    }

    fn landed(&self) -> Vec<Landed> {
        self.git(&["log", "origin/main", LANDED_HISTORY, "--format=%ct%x09%s"])
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let (at, subject) = line.split_once('\t')?;
                Some(Landed { at: at.parse().ok()?, subject: subject.to_owned() })
            })
            .collect()
    }

    fn fleet_names(&self) -> Vec<String> {
        let listed: Value = self.host.run(&["north", "fleet", "list", "--json"], None, None).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null);
        listed
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| row["status"].as_str() != Some("exited"))
            .flat_map(|row| {
                let tasks = row["tasks"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned);
                row["name"].as_str().map(str::to_owned).into_iter().chain(tasks).collect::<Vec<_>>()
            })
            .collect()
    }

    fn lane_names(&self, now: f64) -> Vec<String> {
        let worktrees = self.places.home.join("code").join(PLAN_REPO).join("worktrees");
        let recent = subdirectories(&worktrees).into_iter().filter(|path| modified(path).is_some_and(|at| now - at <= LANE_RECENT_S));
        let tickets = subdirectories(&self.places.state.join("north/land-queue")).into_iter().flat_map(|queue| {
            let outbox = queue.join("outbox");
            files(&queue, "json").into_iter().chain(files(&outbox, "json"))
        });
        recent.chain(tickets).filter_map(|path| path.file_stem().map(|name| name.to_string_lossy().into_owned())).collect()
    }

    fn queue_names(&self) -> Vec<String> {
        self.host.run(&["threads", "queue"], None, None).unwrap_or_default().lines().filter(|line| line.split_whitespace().nth(3).is_some_and(|state| QUEUE_LIVE.contains(&state))).map(str::to_owned).collect()
    }

    fn launch_names(&self, now: f64) -> Vec<String> {
        let text = fs::read_to_string(self.places.state.join("agents/dispatch/launches.jsonl")).unwrap_or_default();
        text.lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|record| record["at"].as_f64().is_some_and(|at| now - at <= DISPATCH_RECENT_S))
            .flat_map(|record| [record["name"].as_str(), record["item"].as_str()].into_iter().flatten().map(str::to_owned).collect::<Vec<_>>())
            .collect()
    }

    fn unowned_boxes(&self) -> Vec<IssueBox> {
        let repos = super::issue_stall_repos();
        let listed = self.host.run(&["threads", "unowned"], None, None).unwrap_or_default();
        let unowned: Vec<(String, String)> = listed
            .lines()
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .filter(|fields| fields.len() >= 3 && fields[2] == "UNOWNED" && repos.iter().any(|repo| fields[0].starts_with(&format!("{repo}#"))))
            .map(|fields| (fields[0].to_owned(), fields.last().copied().unwrap_or_default().to_owned()))
            .collect();
        let mut found = Vec::new();
        for repo in &repos {
            let slug = format!("{}/{repo}", crate::dispatch::OWNER);
            let Some(Value::Array(issues)) = self.host.json(&["gh", "issue", "list", "-R", &slug, "--state", "open", "--limit", "300", "--json", "number,updatedAt"]) else { continue };
            for issue in issues {
                let reference = format!("{repo}#{}", issue["number"].as_u64().unwrap_or_default());
                let touched = issue["updatedAt"].as_str().and_then(crate::foundation::clock::Timestamp::parse_python_datetime).map(|at| at.unix_seconds_f64());
                if let (Some((_, title)), Some(touched)) = (unowned.iter().find(|(found, _)| *found == reference), touched) {
                    found.push(IssueBox { reference, title: title.clone(), ready_since: touched });
                }
            }
        }
        found
    }

    pub fn plans(&self, now: f64) -> plan::Facts {
        let names = self.fleet_names().into_iter().chain(self.lane_names(now)).chain(self.queue_names()).chain(self.launch_names(now)).collect();
        plan::Facts { docs: self.docs(), landed: self.landed(), names, issues: self.unowned_boxes() }
    }

    fn written(&self, declared: &Declared) -> Option<f64> {
        match &declared.origin {
            Origin::File => modified(&self.places.state.join(&declared.name)),
            Origin::Query { database, query } => {
                let path = if database == "ledger" { self.places.ledger.clone() } else { self.places.state.join(database) };
                if !path.exists() {
                    return None;
                }
                let rows = sqlite::rows(&path, Access::ReadOnly, Duration::from_secs(10), query).ok()?;
                let value = rows.first()?.as_object()?.values().next()?.clone();
                match value {
                    Value::Number(number) => number.as_f64(),
                    Value::String(text) => text.parse::<f64>().ok().or_else(|| crate::foundation::clock::Timestamp::parse_python_datetime(&text).map(|at| at.unix_seconds_f64())),
                    _ => None,
                }
            }
        }
    }

    pub fn corrections(&self) -> correction::Facts {
        correction::Facts { seen: crate::threads::incidents::corrections::seen(&self.places.state.join(GROUNDING_LEDGER)) }
    }

    pub fn signals(&self) -> signal::Facts {
        let Ok(doc) = fs::read_to_string(self.checkout().join("docs/feedback-loop.md")) else { return signal::Facts::default() };
        let declared = loops::declared_files(&doc).into_iter().chain(loops::declared_readings(&doc));
        signal::Facts { signals: declared.map(|declared| Signal { last_written: self.written(&declared), name: declared.name, writer: declared.writer, cadence_s: declared.cadence_s, owner: declared.owner }).collect() }
    }

    fn ticket_refusals(&self, since: f64) -> Vec<Refusal> {
        let mut found = Vec::new();
        for queue in subdirectories(&self.places.state.join("north/land-queue")) {
            for path in files(&queue.join("done"), "json") {
                let Some(at) = modified(&path).filter(|at| *at >= since) else { continue };
                let Some(ticket) = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()) else { continue };
                let verdict = ticket["verdict"].as_str().unwrap_or_default();
                if verdict.starts_with("failed ") || verdict.starts_with("conflict ") {
                    found.push(Refusal { lane: ticket["branch"].as_str().unwrap_or_default().to_owned(), sha: ticket["head"].as_str().unwrap_or_default().to_owned(), at, text: with_log(verdict) });
                }
            }
        }
        found
    }

    fn push_refusals(&self, since: f64) -> Vec<Refusal> {
        let text = fs::read_to_string(self.places.state.join("safe-push/events.jsonl")).unwrap_or_default();
        text.lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|event| matches!(event["status"].as_str(), Some("refused" | "failed")) && event["time"].as_f64().is_some_and(|at| at >= since) && !event["sha"].as_str().unwrap_or_default().is_empty())
            .map(|event| Refusal { lane: event["lane"].as_str().unwrap_or_default().to_owned(), sha: event["sha"].as_str().unwrap_or_default().to_owned(), at: event["time"].as_f64().unwrap_or_default(), text: with_log(event["detail"].as_str().unwrap_or_default()) })
            .collect()
    }

    pub fn disks(&self) -> disk::Facts {
        let owner = fs::read_to_string(self.checkout().join(format!("docs/rationale/{DISK_DOC}.md"))).ok().and_then(|text| owner_line(&text));
        let readings = crate::ops::flow::read_hosts(self.host, &crate::pace::target::policy_path());
        disk::Facts { owner, hosts: readings.into_iter().map(|reading| disk::Host { name: reading.host, free_gib: reading.disk_free_gib }).collect() }
    }

    pub fn refusals(&self, now: f64, window_s: f64) -> refusal::Facts {
        let since = now - window_s;
        let owner = fs::read_to_string(self.checkout().join(format!("docs/rationale/{REFUSAL_DOC}.md"))).ok().and_then(|text| owner_line(&text));
        let mut refusals = self.ticket_refusals(since);
        refusals.extend(self.push_refusals(since));
        refusal::Facts { owner, refusals }
    }
}

impl Gatherer<'_> {
    fn after_landing_logs(&self) -> Vec<PathBuf> {
        let mut logs: Vec<(f64, PathBuf)> = subdirectories(&self.places.state.join("north/land-queue"))
            .into_iter()
            .flat_map(|queue| files(&queue.join("logs"), "log"))
            .filter(|path| path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("post-land-") && !name.contains("-on-")) && path.with_extension("outcome").is_file())
            .filter_map(|path| Some((modified(&path)?, path)))
            .collect();
        logs.sort_by(|a, b| a.0.total_cmp(&b.0));
        logs.split_off(logs.len().saturating_sub(RUN_HISTORY)).into_iter().map(|(_, path)| path).collect()
    }

    fn owner_documents(&self) -> Vec<slow_test::Document> {
        files(&self.checkout().join("docs/rationale"), "md").into_iter().filter_map(|path| slow_test::document(&fs::read_to_string(path).ok()?)).collect()
    }

    fn test_file(&self, crossing: &slow_test::Crossing) -> Option<String> {
        let leaf = crossing.name.rsplit("::").next().filter(|leaf| !leaf.is_empty() && leaf.chars().all(|character| character.is_ascii_alphanumeric() || character == '_'))?;
        let listed = self.git(&["grep", "-l", "-E", &format!(r"fn {leaf}\b"), "--", "*.rs"])?;
        slow_test::pick(&listed.lines().collect::<Vec<_>>(), &crossing.binary, &crossing.name)
    }

    fn profile(&self, crossing: &slow_test::Crossing) -> Option<String> {
        let command = slow_test::profile_command(&crossing.binary, &crossing.name)?;
        let seconds = default_number("detector_slow_test_profile_seconds") as u64;
        let head = self.git(&["rev-parse", "origin/main"])?.trim().to_owned();
        let argv = ["north", "capacity", "offload", "run", "--host", &default_text("detector_slow_test_profile_host"), "--base", &head, "--head", &head, "--timeout", &seconds.to_string(), "--", &command];
        let output = self.host.output(&argv, Some(&self.checkout()), None, Duration::from_secs(seconds) + PROFILE_GRACE)?;
        slow_test::summarize(&format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)))
    }

    fn attempts_path(&self) -> PathBuf {
        self.places.flow().join(ATTEMPTS)
    }

    fn attempts(&self) -> BTreeMap<String, f64> {
        fs::read_to_string(self.attempts_path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    fn attempted(&self, attempts: &BTreeMap<String, f64>) {
        let _ = fs::create_dir_all(self.places.flow());
        if let Err(error) = serde_json::to_string(attempts).map_err(std::io::Error::other).and_then(|text| crate::ops::flow::write_atomic(&self.attempts_path(), &text)) {
            eprintln!("detectors: could not record the profile attempt: {error}");
        }
    }

    pub fn slow_tests(&self, limits: &Limits, now: f64) -> slow_test::Facts {
        let runs: Vec<slow_test::Run> = self.after_landing_logs().iter().map(|log| slow_test::passed(&fs::read_to_string(log).unwrap_or_default())).collect();
        let filed = Filer::new(self.places, self.host).filed_keys();
        let documents = self.owner_documents();
        let fallback = default_text("detector_slow_test_owner");
        let mut found: Vec<(slow_test::Crossing, String)> = slow_test::crossing(&runs, limits)
            .into_iter()
            .filter(|crossing| !filed.contains(&slow_test::key(&crossing.binary, &crossing.name)))
            .filter_map(|crossing| {
                let file = self.test_file(&crossing)?;
                Some((crossing, file))
            })
            .collect();
        let mut attempts = self.attempts();
        if self.profiling {
            let tried = |crossing: &slow_test::Crossing, attempts: &BTreeMap<String, f64>| attempts.get(&slow_test::key(&crossing.binary, &crossing.name)).copied().unwrap_or_default();
            found.sort_by(|a, b| tried(&a.0, &attempts).total_cmp(&tried(&b.0, &attempts)));
            found.truncate(PROFILES_PER_PASS);
        }
        let mut slow = Vec::new();
        for (crossing, file) in found {
            let profile = if self.profiling {
                attempts.insert(slow_test::key(&crossing.binary, &crossing.name), now);
                self.attempted(&attempts);
                let Some(profile) = self.profile(&crossing) else { continue };
                profile
            } else {
                DRY_RUN_PROFILE.to_owned()
            };
            slow.push(slow_test::Slow { owner: slow_test::owner_of(&file, &documents, &fallback), file, profile, crossing });
        }
        slow_test::Facts { slow }
    }
}
