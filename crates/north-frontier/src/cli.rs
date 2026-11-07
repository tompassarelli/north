use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use north_base::error::NorthError;
use north_fleet::fleet::model::ApiKey;
use north_providers::ledger::Ledger;
use north_providers::providers::claude::api_pool;

use super::billing::{self, Spent};
use super::grade::{Grade, grade};
use super::sandbox::{Sandbox, git_text};
use super::session::{self, Attempt, Limits, RunRecord};
use super::t3code::{self, Install};
use super::task::{self, Task};
use super::{home, oracle, refusal};
use crate::report::render;
use crate::rows::{Arm, Run};

#[derive(Default)]
struct Flags {
    positional: Vec<String>,
    values: Vec<(String, String)>,
}

impl Flags {
    fn parse(arguments: &[String]) -> Self {
        let mut flags = Self::default();
        let mut words = arguments.iter();
        while let Some(word) = words.next() {
            match word.strip_prefix("--") {
                Some(name) => flags.values.push((name.to_owned(), words.next().cloned().unwrap_or_default())),
                None => flags.positional.push(word.clone()),
            }
        }
        flags
    }

    fn all(&self, name: &str) -> Vec<String> {
        self.values.iter().filter(|(key, _)| key == name).map(|(_, value)| value.clone()).collect()
    }

    fn one(&self, name: &str) -> Option<&str> {
        self.values.iter().rev().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    fn path(&self, name: &str) -> Result<PathBuf, NorthError> {
        self.one(name).map(PathBuf::from).ok_or_else(|| refusal(String::new()))
    }

    fn number(&self, name: &str, default: f64) -> Result<f64, NorthError> {
        self.one(name).map_or(Ok(default), |text| text.parse().map_err(|_| refusal(format!("--{name} takes a number, not {text}"))))
    }

    fn tasks(&self) -> Result<Vec<Task>, NorthError> {
        let file = self.positional.get(1).ok_or_else(|| refusal(String::new()))?;
        let tasks = task::load(Path::new(file))?;
        let only = self.all("only");
        Ok(task::chosen(&tasks, &only).into_iter().cloned().collect())
    }
}

/// A bare mirror of the source repository under the output directory, so neither the oracle's
/// checkouts nor the sandboxes ever touch the primary checkout.
fn mirror(flags: &Flags, out: &Path) -> Result<PathBuf, NorthError> {
    let source = flags.one("source").map_or_else(|| home().join("code/north/main"), PathBuf::from);
    let mirror = out.join("source.git");
    if mirror.exists() {
        git_text(&mirror, &["fetch", "--quiet", "--prune"])?;
    } else {
        fs::create_dir_all(out).map_err(NorthError::Io)?;
        git_text(out, &["clone", "--quiet", "--mirror", "--no-local", &source.display().to_string(), "source.git"])?;
    }
    Ok(mirror)
}

fn run_dir(out: &Path, task: &Task, harness: Arm, sample: u32) -> PathBuf {
    out.join(&task.id).join(format!("{}-{sample}", harness.label()))
}

fn spent(out: &Path) -> f64 {
    let Ok(tasks) = fs::read_dir(out) else { return 0.0 };
    tasks
        .flatten()
        .filter_map(|task| fs::read_dir(task.path()).ok())
        .flatten()
        .flatten()
        .filter_map(|arm| fs::read_to_string(session::record_path(&arm.path())).ok())
        .filter_map(|text| serde_json::from_str::<RunRecord>(&text).ok())
        .map(|record| record.cost_usd)
        .sum()
}

/// Tasks in a seeded shuffle, each task's arms back to back in a seeded order, so both arms of a
/// task run in the same window under the same load.
fn schedule(tasks: &[Task], arms: &[Arm]) -> Vec<(Task, Arm)> {
    let mut keyed: Vec<(String, &Task)> = tasks.iter().map(|task| (task::sha256(&format!("730:{}", task.id)), task)).collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed
        .into_iter()
        .flat_map(|(key, task)| {
            let mut order = arms.to_vec();
            if key.as_bytes()[0] % 2 == 1 {
                order.reverse();
            }
            order.into_iter().map(move |harness| (task.clone(), harness))
        })
        .collect()
}

fn run(flags: &Flags) -> Result<String, NorthError> {
    let tasks = flags.tasks()?;
    let (out, briefs) = (flags.path("out")?, flags.path("briefs")?);
    let arms: Vec<Arm> = match flags.all("arm") {
        named if named.is_empty() => vec![Arm::North, Arm::Plain],
        named => named.iter().map(|name| Arm::parse(name).ok_or_else(|| refusal(format!("no arm named {name}")))).collect::<Result<_, _>>()?,
    };
    let limits = Limits { budget_usd: flags.number("budget-usd", 12.0)?, wall: Duration::from_secs_f64(flags.number("wall-min", 90.0)? * 60.0), idle: Duration::from_secs_f64(flags.number("idle-min", 10.0)? * 60.0) };
    let stop = flags.number("stop-usd", 212.0)?;
    let sample = flags.number("sample", 1.0)? as u32;
    let plan = flags.one("plan").map(|label| { let file = state_root().join("claude-tokens").join(label); std::fs::canonicalize(&file).unwrap_or(file) });
    let t3 = flags.one("t3").map(|dir| Install::new(dir, flags.one("t3-node")));
    if arms.contains(&Arm::T3Code) && t3.is_none() {
        return Err(refusal("--arm t3code needs --t3 DIR (a built T3 Code checkout) and --t3-node PATH"));
    }
    let source = mirror(flags, &out)?;
    let queue = schedule(&tasks, &arms);
    let next = AtomicUsize::new(0);
    let log = Mutex::new(String::new());
    let jobs = flags.number("jobs", 2.0)? as usize;
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1) {
            scope.spawn(|| {
                while let Some((task, harness)) = queue.get(next.fetch_add(1, Ordering::SeqCst)) {
                    let line = job(task, *harness, sample, &out, &briefs, &source, &limits, stop, t3.as_ref(), plan.as_deref());
                    eprintln!("{line}");
                    let _ = writeln!(log.lock().unwrap_or_else(std::sync::PoisonError::into_inner), "{line}");
                }
            });
        }
    });
    let mut text = log.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = writeln!(text, "spent ${:.2} in {}", spent(&out), out.display());
    Ok(text)
}

#[allow(clippy::too_many_arguments, reason = "one job's inputs, all owned by the run loop")]
fn job(task: &Task, harness: Arm, sample: u32, out: &Path, briefs: &Path, source: &Path, limits: &Limits, stop: f64, t3: Option<&Install>, plan: Option<&Path>) -> String {
    let root = run_dir(out, task, harness, sample);
    if session::record_path(&root).exists() {
        return format!("{} {}: already run", task.id, harness.label());
    }
    if spent(out) >= stop {
        return format!("{} {}: not started, spend reached ${stop:.2}", task.id, harness.label());
    }
    let result = super::task::brief(briefs, task).and_then(|brief: String| {
        let mut refused = Vec::new();
        loop {
            set_aside(&root)?;
            let (org, key) = match plan {
                Some(file) => (String::new(), ApiKey { org: String::new(), file: file.to_owned() }),
                None => pool_key(&refused)?,
            };
            let sandbox = Sandbox::build(&root, task, source, harness)?;
            let attempt = Attempt { task, harness, sample, key: &key.file, plan: plan.is_some(), brief: &brief, limits };
            let (record, spent) = match t3 {
                Some(install) if harness == Arm::T3Code => t3code::run(&attempt, &sandbox, install)?,
                _ => session::run(&attempt, &sandbox)?,
            };
            if plan.is_some() && record.ended == session::CREDIT {
                return Err(refusal("the plan account refused the run: its window is at the limit"));
            }
            if plan.is_none() {
                charge_ledger(&key.org, &spent);
            }
            if record.ended == session::CREDIT {
                let _ = api_pool::mark_ran_out(&north_base::platform::orchestration::path(), &state_root(), &org, &record.session);
                if !refused.is_empty() {
                    return Err(refusal("the API refused the run for credit on two orgs in a row"));
                }
                refused.push(org);
                continue;
            }
            fs::write(session::record_path(&root), serde_json::to_string_pretty(&record).map_err(NorthError::Json)?).map_err(NorthError::Io)?;
            return Ok(record);
        }
    });
    match result {
        Ok(record) => format!("{} {}: {} in {:.0} min, ${:.2}, {} interventions", task.id, harness.label(), record.ended, record.wall_s / 60.0, record.cost_usd, record.interventions),
        Err(error) => format!("{} {}: failed: {error}", task.id, harness.label()),
    }
}

fn charge_ledger(org: &str, spent: &Spent) {
    let charged = Ledger::open(&state_root().join("ledger.db")).and_then(|ledger| billing::charge(&ledger, org, spent));
    if let Err(error) = charged {
        eprintln!("north test frontier: the spend of {} on api:{org} was not written to the ledger: {}", spent.name, error.user_message());
    }
}

fn state_root() -> PathBuf {
    home().join(".local/state/north")
}

fn pool_key(refused: &[String]) -> Result<(String, ApiKey), NorthError> {
    api_pool::next_key(&north_base::platform::orchestration::path(), &home(), &state_root(), &north_base::platform::host::name(), refused).ok_or_else(|| refusal("no api org in the pool has credit left"))
}

fn set_aside(root: &Path) -> Result<(), NorthError> {
    if !root.exists() {
        return Ok(());
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    fs::rename(root, root.with_extension(format!("failed-{stamp}"))).map_err(|error| refusal(format!("an unfinished run is in the way: {error}")))
}

fn arm_dirs(out: &Path, task: &Task) -> Vec<(Arm, PathBuf)> {
    let Ok(entries) = fs::read_dir(out.join(&task.id)) else { return Vec::new() };
    let mut found: Vec<(Arm, PathBuf)> = entries.flatten().filter_map(|entry| Some((Arm::parse(entry.file_name().to_str()?.split('-').next()?)?, entry.path()))).filter(|(_, dir)| session::record_path(dir).exists()).collect();
    found.sort_by(|a, b| a.1.cmp(&b.1));
    found
}

fn grade_all(flags: &Flags) -> Result<String, NorthError> {
    let tasks = flags.tasks()?;
    let out = flags.path("out")?;
    let source = mirror(flags, &out)?;
    let mut text = String::new();
    for task in &tasks {
        let oracle = match oracle::build(task, &source, &out.join(&task.id).join("oracle"), &out.join("oracle-target")) {
            Ok(oracle) => oracle,
            Err(error) => {
                let _ = writeln!(text, "{}: no oracle: {error}", task.id);
                continue;
            }
        };
        for (harness, dir) in arm_dirs(&out, task) {
            let file = dir.join("grade.json");
            let graded = match fs::read_to_string(&file) {
                Ok(saved) => serde_json::from_str::<Grade>(&saved).map_err(NorthError::Json),
                Err(_) => grade(task, &Sandbox::at(&dir, &task.repo), &source, &oracle).and_then(|graded| {
                    fs::write(&file, serde_json::to_string_pretty(&graded).map_err(NorthError::Json)?).map_err(NorthError::Io)?;
                    Ok(graded)
                }),
            };
            let _ = match graded {
                Ok(graded) => writeln!(text, "{} {}: correct={} fail-to-pass {}/{} broken {} leaks {:?}", task.id, harness.label(), graded.correct, graded.fail_to_pass_passed, graded.fail_to_pass, graded.broken.len(), graded.leaks),
                Err(error) => writeln!(text, "{} {}: not graded: {error}", task.id, harness.label()),
            };
        }
    }
    Ok(text)
}

fn oracles(flags: &Flags) -> Result<String, NorthError> {
    let tasks = flags.tasks()?;
    let out = flags.path("out")?;
    let source = mirror(flags, &out)?;
    let mut text = String::new();
    for task in &tasks {
        let _ = match oracle::build(task, &source, &out.join(&task.id).join("oracle"), &out.join("oracle-target")) {
            Ok(found) => writeln!(text, "{}: fail-to-pass {}, passing at the landing {}, at the base {}", task.id, found.fail_to_pass.len(), found.reference.len(), found.base.len()),
            Err(error) => writeln!(text, "{}: no oracle: {error}", task.id),
        };
    }
    Ok(text)
}

fn rows(flags: &Flags) -> Result<Vec<Run>, NorthError> {
    let tasks = flags.tasks()?;
    let out = flags.path("out")?;
    let mut rows = Vec::new();
    for task in &tasks {
        for (_, dir) in arm_dirs(&out, task) {
            let (Ok(run), Ok(graded)) = (fs::read_to_string(session::record_path(&dir)), fs::read_to_string(dir.join("grade.json"))) else { continue };
            let run: RunRecord = serde_json::from_str(&run).map_err(NorthError::Json)?;
            let graded: Grade = serde_json::from_str(&graded).map_err(NorthError::Json)?;
            rows.push(Run { task: task.id.clone(), arm: run.arm, model: run.model, effort: run.effort, correct_first_pass: graded.correct && run.interventions == 0, correct: graded.correct, cost_usd: run.cost_usd, wall_seconds: run.wall_s, interventions: run.interventions, contaminated: !graded.leaks.is_empty() });
        }
    }
    Ok(rows)
}

fn report(flags: &Flags) -> Result<String, NorthError> {
    let rows = rows(flags)?;
    if let Some(file) = flags.one("rows") {
        let lines: Vec<String> = rows.iter().map(serde_json::to_string).collect::<Result<_, _>>().map_err(NorthError::Json)?;
        fs::write(file, lines.join("\n") + "\n").map_err(NorthError::Io)?;
    }
    Ok(render(&rows))
}

fn candidates(flags: &Flags) -> Result<String, NorthError> {
    let ledger = flags.one("ledger").map_or_else(|| home().join(".local/state/threads/snapshot.db"), PathBuf::from);
    let (since, until) = (flags.one("since").ok_or_else(|| refusal(String::new()))?, flags.one("until").ok_or_else(|| refusal(String::new()))?);
    let tasks = task::candidates(&ledger, &flags.path("repo")?, since, until, &flags.path("briefs")?)?;
    Ok(serde_json::to_string_pretty(&tasks).map_err(NorthError::Json)? + "\n")
}

pub fn dispatch(arguments: &[String]) -> Result<String, NorthError> {
    let flags = Flags::parse(arguments);
    match flags.positional.first().map(String::as_str) {
        Some("tasks") => candidates(&flags),
        Some("oracle") => oracles(&flags),
        Some("run") => run(&flags),
        Some("grade") => grade_all(&flags),
        Some("report") => report(&flags),
        _ => Err(refusal(String::new())),
    }
}
