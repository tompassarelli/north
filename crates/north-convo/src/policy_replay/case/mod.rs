mod appended;
mod arm;
mod judge;
mod source;
mod spec;

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
use self::spec::Spec;
use super::events::{parse_events, tool_calls, usage};
use super::group::reap;
use super::json::Json;
use super::run::{run_tagged, state_dir};
use crate::error::NorthError;
use crate::fleet::install;
use crate::fleet::prompt::prompt_sha;
use crate::foundation::clock::Timestamp;
use crate::platform::replay_verdict::{self, Verdict};

const USAGE: &str = "usage: north policy-replay case CASE... [--arm NAME=GENERATION]... [--samples N] [--jobs N] [--out ROWS.jsonl] [--judge-model M] [--max-turns N] [--budget-usd N] [--dry-run]
  CASE is a correction id from the grounding ledger or a case file (harness/cases/*.toml).
  GENERATION is own (the agent generation the session ran under), current, a gen-* directory, or REPO@REV: the session's
  own generation with the skills, CLAUDE.md and agent definitions that REPO (north or south) holds at revision REV.
  A case cut from a north fleet conversation (found by its record, or named by fleet_session in the case file) resumes
  with that session's fleet prompt appended, rendered from the arm's prompt.md (north@REV takes it at REV) with the session's standing orders up to the cut and its brief;
  --dry-run prints each arm's appended-prompt hash and runs nothing.
  The first arm is the base. Each case runs --samples per arm (default 2, and a third when the two disagree); with two
  arms a judged case prints kept or regressed only when the arms differ at the 90% level, else inconclusive. A case file
  replayed with arms named base and lane gets its graded samples in CASE.proof.jsonl beside it, the proof
  `north land scan detector-proof` reads.";

struct Options {
    ids: Vec<String>,
    arms: Vec<Arm>,
    jobs: usize,
    out: Option<PathBuf>,
    judge: String,
    max_turns: u32,
    budget: f64,
    samples: usize,
    dry_run: bool,
}

fn parse(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options { ids: Vec::new(), arms: Vec::new(), jobs: 3, out: None, judge: "claude-sonnet-5-5".into(), max_turns: 40, budget: 10.0, samples: 2, dry_run: false };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let mut value = |flag: &str| rest.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        match argument.as_str() {
            "--arm" => options.arms.push(arm::parse(&value("--arm")?)),
            "--samples" => options.samples = value("--samples")?.parse().map_err(|_| "--samples needs a whole number")?,
            "--jobs" => options.jobs = value("--jobs")?.parse().map_err(|_| "--jobs needs a whole number")?,
            "--out" => options.out = Some(PathBuf::from(value("--out")?)),
            "--judge-model" => options.judge = value("--judge-model")?,
            "--max-turns" => options.max_turns = value("--max-turns")?.parse().map_err(|_| "--max-turns needs a whole number")?,
            "--budget-usd" => options.budget = value("--budget-usd")?.parse().map_err(|_| "--budget-usd needs a number")?,
            "--dry-run" => options.dry_run = true,
            flag if flag.starts_with("--") => {
                return Err(format!("unrecognized arguments: {flag}"));
            }
            id => options.ids.push(id.to_owned()),
        }
    }
    if options.ids.is_empty() {
        return Err("name at least one correction id or case file".into());
    }
    if options.arms.is_empty() {
        options.arms.push(arm::parse("own"));
    }
    Ok(options)
}

struct Replayed {
    name: String,
    generation: Generation,
    transcript: String,
    substitutions: usize,
    appended: Option<String>,
}

struct Prepared {
    case: Case,
    spec: Spec,
    arms: Vec<Replayed>,
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
    let (case, spec) = spec::load(id)?;
    let own = arm::generation("own", &case)?;
    let launch = appended::launch(&case, spec.fleet_session.as_deref());
    let mut prepared = Vec::new();
    for arm in arms {
        let generation = arm::generation(&arm.spec, &case)?;
        let pairs = own.substitutions(&generation);
        let (transcript, count) = arm::transcript(&case.chain, &pairs);
        if !pairs.is_empty() && count == 0 {
            eprintln!("policy-replay case: {id} arm {}: {} differs from {} but none of its changed text appears in the transcript", arm.name, generation.name(), own.name());
        }
        let appended = launch.as_ref().map(|launch| launch.appended(generation.fleet_template()));
        prepared.push(Replayed { name: arm.name.clone(), generation, transcript, substitutions: count, appended });
    }
    eprintln!("policy-replay case: {id} {} {} cut at {} in {} (base {})", spec.kind.word(), case.class, case.cut_at, case.session, prepared[0].generation.name());
    Ok(Prepared { case, spec, arms: prepared })
}

fn sample(prepared: &Prepared, job: &Job, options: &Options, env: &[(String, String)]) -> Result<Value, NorthError> {
    let case = &prepared.case;
    let Replayed { name, generation, transcript, substitutions: count, appended } = &prepared.arms[job.arm];
    let started = Instant::now();
    let root = tempfile::Builder::new().prefix("policy-replay-case-").tempdir().map_err(NorthError::Io)?;
    arm::config_dir(root.path(), case, generation, transcript)?;
    let mut command = Command::new(install::runtime("claude"));
    command.args(["-p", &case.prompt, "--resume", &case.session, "--fork-session", "--system-prompt-snapshot", "off"]);
    command.args(["--model", &case.model, "--effort", &case.effort, "--max-turns", &options.max_turns.to_string(), "--max-budget-usd", &options.budget.to_string()]);
    if let Some(text) = appended {
        command.args(["--append-system-prompt", text]);
    }
    command.args(["--permission-mode", "dontAsk", "--setting-sources", "user", "--strict-mcp-config", "--output-format", "stream-json", "--verbose"]);
    command.env_clear().envs(env.iter().cloned()).env("CLAUDE_CONFIG_DIR", root.path()).env("CLAUDE_CODE_DISABLE_ADVISOR_TOOL", "1");
    command.current_dir(if case.cwd.is_dir() { case.cwd.clone() } else { root.path().to_path_buf() });
    let mut run = run_tagged(command, 1800.0)?;
    let evs = parse_events(&run.stdout);
    let leftover = reap(&run.tag, &mut run.child);
    let calls: Vec<Value> = tool_calls(&evs).iter().map(|call| json!({ "name": call.text("name"), "input": call.field("input").dumps() })).collect();
    let answer = replay_text(&evs);
    let used = usage(&evs);
    let mut row = json!({
        "case": case.id, "kind": prepared.spec.kind.word(), "class": case.class, "session": case.session, "cut_at": case.cut_at,
        "arm": name, "generation": generation.name(), "substitutions": count, "sample": job.sample,
        "model": case.model, "effort": case.effort, "detector": if prepared.spec.detector.deterministic() { "pattern".to_owned() } else { options.judge.clone() },
        "appended_sha": appended.as_deref().map(prompt_sha), "tool_calls": calls, "final_text": answer, "leftover_processes": leftover,
    });
    match &used {
        Some(used) if used.subtype.as_deref() == Some("success") || !answer.is_empty() => {
            let lines: Vec<String> = calls.iter().map(|call| format!("{} {}", call["name"].as_str().unwrap_or(""), call["input"].as_str().unwrap_or(""))).chain([answer.clone()]).collect();
            let grade = match prepared.spec.detector.repeats(&lines) {
                Some(repeats) => judge::Grade { repeats: Some(repeats), line: "pattern detector".into(), usd: 0.0 },
                None => judge::grade(case, &calls, &answer, &options.judge, env)?,
            };
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

fn lane_verdicts(spec: &Spec, id: &str, rows: &[Value], arms: &[&str]) -> Vec<Option<Verdict>> {
    let Some((base, lanes)) = arms.split_first() else { return Vec::new() };
    let base = graded(rows, id, base);
    lanes.iter().map(|lane| replay_verdict::decide(spec.kind, base, graded(rows, id, lane), spec.detector.deterministic())).collect()
}

struct Report {
    lines: Vec<String>,
    reproduced: usize,
    refused: Vec<String>,
}

fn report(prepared: &[Prepared], rows: &[Value]) -> Report {
    let mut lines = Vec::new();
    let mut reproduced = 0;
    let mut refused = Vec::new();
    for entry in prepared {
        let id = &entry.case.id;
        let mut parts = Vec::new();
        let mut states = Vec::new();
        let mut counts = Vec::new();
        for Replayed { name, generation, substitutions: count, .. } in &entry.arms {
            let (repeats, of) = graded(rows, id, name);
            let state = reproduces(repeats, of);
            let word = match state {
                Some(true) => "reproduces",
                Some(false) => "avoids",
                None => "ungraded",
            };
            parts.push(format!("{name} ({}, {count} substitutions) {repeats}/{of} {word}", generation.name()));
            states.push(state);
            counts.push((repeats, of));
        }
        if states.first().copied().flatten() == Some(true) {
            reproduced += 1;
        }
        let names: Vec<&str> = entry.arms.iter().map(|arm| arm.name.as_str()).collect();
        let verdicts = lane_verdicts(&entry.spec, id, rows, &names);
        for (name, verdict) in names[1..].iter().zip(&verdicts) {
            if verdict.as_ref().is_some_and(Verdict::refuses) {
                refused.push(format!("{name} regresses {} case {id}", entry.spec.kind.word()));
            }
        }
        let verdict = match (states.first().copied().flatten(), counts.len()) {
            (Some(true), 1) => "reproduced".to_owned(),
            (Some(false), 1) => "not reproduced".to_owned(),
            (_, 2..) => names[1..].iter().zip(&verdicts).map(|(name, verdict)| format!("{name} {}", verdict.as_ref().map_or_else(|| "ungraded".to_owned(), Verdict::line))).collect::<Vec<_>>().join("; "),
            _ => "ungraded".to_owned(),
        };
        lines.push(format!("{id} {} {}: {} -> {verdict}", entry.spec.kind.word(), entry.case.class, parts.join("; ")));
    }
    Report { lines, reproduced, refused }
}

fn dry_run(options: &Options) -> Result<u8, NorthError> {
    for id in &options.ids {
        let entry = prepare(id, &options.arms)?;
        for arm in &entry.arms {
            let appended = arm.appended.as_ref().map_or_else(
                || format!("no appended prompt: no north fleet record owns session {} (name it with fleet_session in the case file)", entry.case.session),
                |text| format!("appended prompt sha256 {} ({} bytes)", prompt_sha(text), text.len()),
            );
            println!("{} {} ({}): {appended}", entry.case.id, arm.name, arm.generation.name());
        }
    }
    Ok(0)
}

fn replayed(options: &Options) -> Result<u8, NorthError> {
    if options.dry_run {
        return dry_run(options);
    }
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
    let first: Vec<Job> = (0..prepared.len()).flat_map(|case| (0..options.arms.len()).flat_map(move |arm| (0..options.samples).map(move |sample| Job { case, arm, sample }))).collect();
    let mut rows = run_jobs(&prepared, &first, options, &out)?;
    let split: Vec<Job> = (0..prepared.len())
        .flat_map(|case| (0..options.arms.len()).map(move |arm| (case, arm)))
        .filter(|&(case, arm)| {
            let (repeats, of) = graded(&rows, &prepared[case].case.id, &prepared[case].arms[arm].name);
            options.samples == 2 && (of < 2 || (repeats == 1 && of == 2))
        })
        .map(|(case, arm)| Job { case, arm, sample: 2 })
        .collect();
    rows.extend(run_jobs(&prepared, &split, options, &out)?);
    let Report { lines, reproduced, refused } = report(&prepared, &rows);
    write_proofs(options, &prepared, &rows)?;
    for line in lines.iter().chain(&unreplayable) {
        println!("{line}");
    }
    for refusal in &refused {
        println!("refused: {refusal}");
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
    Ok(u8::from(!refused.is_empty()))
}

fn write_proofs(options: &Options, prepared: &[Prepared], rows: &[Value]) -> Result<(), NorthError> {
    use replay_verdict::{BASE_ARM, LANE_ARM, PROOF_SUFFIX, proof_row};
    let named = |arm: &str| options.arms.iter().any(|candidate| candidate.name == arm);
    if !named(BASE_ARM) || !named(LANE_ARM) {
        return Ok(());
    }
    for path in options.ids.iter().filter_map(|id| id.strip_suffix(".toml")) {
        let id = std::path::Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(entry) = prepared.iter().find(|entry| entry.case.id == id) else { continue };
        let deterministic = entry.spec.detector.deterministic();
        let lines: String = rows
            .iter()
            .filter(|row| row["case"] == id.as_str() && (row["arm"] == BASE_ARM || row["arm"] == LANE_ARM))
            .map(|row| proof_row(entry.spec.kind, row["arm"].as_str().unwrap_or(""), row["generation"].as_str().unwrap_or(""), row["sample"].as_u64().unwrap_or(0) as usize, row["repeats"].as_bool(), deterministic).to_string() + "\n")
            .collect();
        let proof = format!("{path}{PROOF_SUFFIX}");
        fs::write(&proof, lines).map_err(NorthError::Io)?;
        println!("proof for {id} in {proof}; commit it beside the case");
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Verdict, lane_verdicts, spec};

    #[test]
    fn a_control_case_refuses_a_lane_that_overcorrects_and_never_proves_one_kept() {
        let planted = "kind = \"control\"\nsession = \"s\"\nmessage = \"m\"\n[detector]\nexpects = \"^Bash cargo test\"\n";
        let (spec, _) = spec::parse("harness/cases/run-the-tests.toml", planted).unwrap();
        let ordinary = ["Bash cargo test -p north-convo".to_owned(), "Done: tests pass.".to_owned()];
        let overcorrecting = ["Blocked: the tests may take a while, so I am asking before running them.".to_owned()];
        let rows: Vec<_> = [("base", &ordinary[..]), ("lane", &overcorrecting[..])]
            .into_iter()
            .flat_map(|(arm, lines)| (0..2).map(move |sample| (arm, lines, sample)))
            .map(|(arm, lines, sample)| json!({"case": "run-the-tests", "arm": arm, "sample": sample, "repeats": spec.detector.repeats(lines)}))
            .collect();
        assert_eq!(lane_verdicts(&spec, "run-the-tests", &rows, &["base", "lane"]), [Some(Verdict::Regressed)]);
        assert!(Verdict::Regressed.refuses());
        assert_eq!(lane_verdicts(&spec, "run-the-tests", &rows, &["lane", "base"]), [Some(Verdict::Held)]);
    }
}
