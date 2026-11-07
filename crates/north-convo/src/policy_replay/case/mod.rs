mod arm;
mod judge;
mod source;

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use serde_json::{Value, json};

use self::arm::{Arm, Generation};
use self::source::Case;
use super::events::{parse_events, tool_calls, usage};
use super::group::reap;
use super::json::Json;
use super::run::{run_tagged, state_dir};
use crate::error::NorthError;
use crate::fleet::install;
use crate::foundation::clock::Timestamp;

const USAGE: &str = "usage: north policy-replay case CORRECTION_ID... [--arm NAME=GENERATION]... [--jobs N] [--out ROWS.jsonl] [--judge-model M] [--max-turns N] [--budget-usd N]
  GENERATION is own (the agent generation the session ran under), current, or a gen-* directory under north-data/agents.
  The first arm is the base; each case runs 2 samples per arm and a third when they disagree.";

struct Options {
    ids: Vec<String>,
    arms: Vec<Arm>,
    jobs: usize,
    out: Option<PathBuf>,
    judge: String,
    max_turns: u32,
    budget: f64,
}

fn parse(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options { ids: Vec::new(), arms: Vec::new(), jobs: 3, out: None, judge: "claude-sonnet-5-5".into(), max_turns: 40, budget: 10.0 };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let mut value = |flag: &str| rest.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        match argument.as_str() {
            "--arm" => options.arms.push(arm::parse(&value("--arm")?)),
            "--jobs" => options.jobs = value("--jobs")?.parse().map_err(|_| "--jobs needs a whole number")?,
            "--out" => options.out = Some(PathBuf::from(value("--out")?)),
            "--judge-model" => options.judge = value("--judge-model")?,
            "--max-turns" => options.max_turns = value("--max-turns")?.parse().map_err(|_| "--max-turns needs a whole number")?,
            "--budget-usd" => options.budget = value("--budget-usd")?.parse().map_err(|_| "--budget-usd needs a number")?,
            flag if flag.starts_with("--") => {
                return Err(format!("unrecognized arguments: {flag}"));
            }
            id => options.ids.push(id.to_owned()),
        }
    }
    if options.ids.is_empty() {
        return Err("name at least one correction id from the grounding ledger".into());
    }
    if options.arms.is_empty() {
        options.arms.push(arm::parse("own"));
    }
    Ok(options)
}

struct Prepared {
    case: Case,
    arms: Vec<(String, Generation, String, usize)>,
}

struct Job {
    case: usize,
    arm: usize,
    sample: usize,
}

fn child_env() -> Vec<(String, String)> {
    std::env::vars().filter(|(name, _)| !name.starts_with("AGENT_") && !name.starts_with("CLAUDE_CODE_") && name != "CLAUDECODE" && name != "CLAUDE_CONFIG_DIR").collect()
}

fn prepare(id: &str, arms: &[Arm]) -> Result<Prepared, NorthError> {
    let case = source::resolve(id)?;
    let base = arm::generation(&arms[0].spec, &case)?;
    let own = arm::generation("own", &case)?;
    let mut prepared = Vec::new();
    for arm in arms {
        let generation = arm::generation(&arm.spec, &case)?;
        let pairs = own.substitutions(&generation);
        let (transcript, count) = arm::transcript(&case.chain, &pairs);
        if !pairs.is_empty() && count == 0 {
            eprintln!("policy-replay case: {id} arm {}: {} differs from {} but none of its changed text appears in the transcript", arm.name, generation.name(), own.name());
        }
        prepared.push((arm.name.clone(), generation, transcript, count));
    }
    eprintln!("policy-replay case: {id} {} cut at {} in {} (base {})", case.class, case.cut_at, case.session, base.name());
    Ok(Prepared { case, arms: prepared })
}

fn sample(prepared: &Prepared, job: &Job, options: &Options, env: &[(String, String)]) -> Result<Value, NorthError> {
    let case = &prepared.case;
    let (name, generation, transcript, count) = &prepared.arms[job.arm];
    let started = Instant::now();
    let root = tempfile::Builder::new().prefix("policy-replay-case-").tempdir().map_err(NorthError::Io)?;
    arm::config_dir(root.path(), case, generation, transcript)?;
    let mut command = Command::new(install::runtime("claude"));
    command.args(["-p", &case.prompt, "--resume", &case.session, "--fork-session", "--system-prompt-snapshot", "off"]);
    command.args(["--model", &case.model, "--effort", &case.effort, "--max-turns", &options.max_turns.to_string(), "--max-budget-usd", &options.budget.to_string()]);
    command.args(["--permission-mode", "dontAsk", "--setting-sources", "user", "--strict-mcp-config", "--output-format", "stream-json", "--verbose"]);
    command.env_clear().envs(env.iter().cloned()).env("CLAUDE_CONFIG_DIR", root.path()).env("CLAUDE_CODE_DISABLE_ADVISOR_TOOL", "1");
    command.current_dir(if case.cwd.is_dir() { case.cwd.clone() } else { root.path().to_path_buf() });
    let mut run = run_tagged(command, 1800.0)?;
    let evs = parse_events(&run.stdout);
    let leftover = reap(&run.tag, &mut run.child);
    let calls: Vec<Value> = tool_calls(&evs).iter().map(|call| json!({ "name": call.text("name"), "input": tail(&call.field("input").dumps(), 400) })).collect();
    let answer = replay_text(&evs);
    let used = usage(&evs);
    let mut row = json!({
        "case": case.id, "class": case.class, "session": case.session, "cut_at": case.cut_at,
        "arm": name, "generation": generation.name(), "substitutions": count, "sample": job.sample,
        "model": case.model, "effort": case.effort, "judge_model": options.judge,
        "tool_calls": calls, "final_text": answer, "leftover_processes": leftover,
    });
    match &used {
        Some(used) if used.subtype.as_deref() == Some("success") || !answer.is_empty() => {
            let grade = judge::grade(case, &calls, &answer, &options.judge, env)?;
            row["repeats"] = json!(grade.repeats);
            row["judge"] = json!(grade.line);
            row["usd"] = json!(used.cost_api_usd_comparison + grade.usd);
            row["turns"] = json!(used.turns);
            row["input_tokens"] = json!(used.input_tokens + used.cache_creation_tokens + used.cache_read_tokens);
        }
        _ => {
            row["repeats"] = Value::Null;
            row["error"] = json!(format!("no answer (exit {}): {}", run.code, tail(&run.stderr, 400)));
            row["usd"] = json!(used.as_ref().map_or(0.0, |used| used.cost_api_usd_comparison));
        }
    }
    row["wall_s"] = json!((started.elapsed().as_secs_f64() * 10.0).round() / 10.0);
    Ok(row)
}

fn tail(text: &str, chars: usize) -> String {
    let skip = text.chars().count().saturating_sub(chars);
    text.chars().skip(skip).collect()
}

fn run_jobs(prepared: &[Prepared], jobs: &[Job], options: &Options, out: &PathBuf) -> Result<Vec<Value>, NorthError> {
    let env = child_env();
    let next = AtomicUsize::new(0);
    let rows = Mutex::new(Vec::new());
    let failure = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.clamp(1, 6) {
            scope.spawn(|| {
                while let Some(job) = jobs.get(next.fetch_add(1, Ordering::SeqCst)) {
                    match sample(&prepared[job.case], job, options, &env) {
                        Ok(row) => {
                            let line = serde_json::to_string(&row).unwrap_or_default();
                            let _ = fs::OpenOptions::new().create(true).append(true).open(out).and_then(|mut file| writeln!(file, "{line}"));
                            eprintln!(
                                "policy-replay case: {} {} sample {}: {} ${:.2} {}",
                                row["case"].as_str().unwrap_or(""),
                                row["arm"].as_str().unwrap_or(""),
                                job.sample,
                                verdict_word(&row["repeats"]),
                                row["usd"].as_f64().unwrap_or(0.0),
                                row["judge"].as_str().or(row["error"].as_str()).unwrap_or("")
                            );
                            rows.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(row);
                        }
                        Err(error) => {
                            failure.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert(error);
                        }
                    }
                }
            });
        }
    });
    if let Some(error) = failure.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner) {
        return Err(error);
    }
    Ok(rows.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner))
}

fn verdict_word(repeats: &Value) -> &'static str {
    match repeats.as_bool() {
        Some(true) => "repeats",
        Some(false) => "avoids",
        None => "ungraded",
    }
}

fn graded(rows: &[Value], case: &str, arm: &str) -> (usize, usize) {
    let marks: Vec<bool> = rows.iter().filter(|row| row["case"] == case && row["arm"] == arm).filter_map(|row| row["repeats"].as_bool()).collect();
    (marks.iter().filter(|mark| **mark).count(), marks.len())
}

fn reproduces(repeats: usize, of: usize) -> Option<bool> {
    (of >= 2).then_some(2 * repeats >= of)
}

fn report(prepared: &[Prepared], rows: &[Value]) -> (Vec<String>, usize) {
    let mut lines = Vec::new();
    let mut reproduced = 0;
    for entry in prepared {
        let id = &entry.case.id;
        let mut parts = Vec::new();
        let mut states = Vec::new();
        for (name, generation, _, count) in &entry.arms {
            let (repeats, of) = graded(rows, id, name);
            let state = reproduces(repeats, of);
            let word = match state {
                Some(true) => "reproduces",
                Some(false) => "avoids",
                None => "ungraded",
            };
            parts.push(format!("{name} ({}, {count} substitutions) {repeats}/{of} {word}", generation.name()));
            states.push(state);
        }
        if states.first().copied().flatten() == Some(true) {
            reproduced += 1;
        }
        let verdict = match (states.first().copied().flatten(), states.get(1).copied().flatten()) {
            (Some(true), None) if states.len() == 1 => "reproduced".to_owned(),
            (Some(false), None) if states.len() == 1 => "not reproduced".to_owned(),
            (Some(true), Some(false)) => "fixed".to_owned(),
            (Some(true), Some(true)) => "still fails".to_owned(),
            (Some(false), Some(true)) => "regressed".to_owned(),
            (Some(false), Some(false)) => "not reproduced on base".to_owned(),
            _ => "ungraded".to_owned(),
        };
        lines.push(format!("{id} {}: {} -> {verdict}", entry.case.class, parts.join("; ")));
    }
    (lines, reproduced)
}

fn replayed(options: &Options) -> Result<u8, NorthError> {
    if std::env::var("ANTHROPIC_API_KEY").map_or(true, |key| key.is_empty()) && std::env::var("CLAUDE_CODE_OAUTH_TOKEN").map_or(true, |token| token.is_empty()) {
        return Err(NorthError::Usage("policy-replay case: replays run in a scratch Claude config, so they need ANTHROPIC_API_KEY or CLAUDE_CODE_OAUTH_TOKEN in the environment".into()));
    }
    let out = options.out.clone().unwrap_or_else(|| state_dir().join(format!("cases-{}.jsonl", Timestamp::now().iso().replace(':', ""))));
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(NorthError::Io)?;
    }
    let mut prepared = Vec::new();
    let mut unreplayable = Vec::new();
    for id in &options.ids {
        match prepare(id, &options.arms) {
            Ok(case) => prepared.push(case),
            Err(NorthError::Usage(message)) => {
                eprintln!("{message}");
                unreplayable.push(format!("{id}: not replayable, {}", message.trim_start_matches("policy-replay case: ")));
            }
            Err(error) => return Err(error),
        }
    }
    let first: Vec<Job> = (0..prepared.len()).flat_map(|case| (0..options.arms.len()).flat_map(move |arm| (0..2).map(move |sample| Job { case, arm, sample }))).collect();
    let mut rows = run_jobs(&prepared, &first, options, &out)?;
    let split: Vec<Job> = (0..prepared.len())
        .flat_map(|case| (0..options.arms.len()).map(move |arm| (case, arm)))
        .filter(|&(case, arm)| {
            let (repeats, of) = graded(&rows, &prepared[case].case.id, &prepared[case].arms[arm].0);
            of < 2 || (repeats == 1 && of == 2)
        })
        .map(|(case, arm)| Job { case, arm, sample: 2 })
        .collect();
    rows.extend(run_jobs(&prepared, &split, options, &out)?);
    let (lines, reproduced) = report(&prepared, &rows);
    for line in lines.iter().chain(&unreplayable) {
        println!("{line}");
    }
    let spent: f64 = rows.iter().filter_map(|row| row["usd"].as_f64()).sum();
    println!(
        "reproduced on {}: {reproduced} of {} cases ({} not replayable); {} samples, ${spent:.2}; rows in {}",
        options.arms[0].name,
        prepared.len() + unreplayable.len(),
        unreplayable.len(),
        rows.len(),
        out.display()
    );
    Ok(0)
}

pub fn run(arguments: &[String]) -> u8 {
    let options = match parse(arguments) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{USAGE}\nnorth policy-replay case: error: {message}");
            return 2;
        }
    };
    match replayed(&options) {
        Ok(code) => code,
        Err(NorthError::Usage(message) | NorthError::Refused(message)) => {
            eprintln!("{message}");
            1
        }
        Err(error) => {
            eprintln!("policy-replay case: {error}");
            1
        }
    }
}

pub fn hold() -> u8 {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    if let Some(decision) = arm::hold(&input) {
        println!("{decision}");
    }
    0
}

fn replay_text(evs: &[Json]) -> String {
    evs.iter()
        .filter(|ev| ev.is_type("assistant"))
        .flat_map(|ev| ev.field("message").field("content").items())
        .filter(|block| block.is_type("text"))
        .filter_map(|block| block.text("text"))
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}
