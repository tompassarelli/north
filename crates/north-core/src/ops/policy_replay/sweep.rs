use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;
use serde_json::Value;

use super::group::install_signal_handlers;
use super::policy::{Variant, arm_minus, no_fable, policy_for_arm, variant_policy};
use super::point::write_candidate;
use super::pyfmt::float_repr;
use super::rows::{RunRow, summary, variant_row};
use super::run::{Item, Pick, run_plan, run_row, state_dir};
use super::stats::operating_point;
use crate::error::NorthError;
use crate::foundation::clock::Timestamp;
use crate::platform::pystats::wilson;

pub struct Common {
    pub scenarios: Vec<PathBuf>,
    pub trials: usize,
    pub jobs: usize,
    pub policy: Option<PathBuf>,
    pub out: Option<PathBuf>,
    pub pick: Pick,
}

pub struct Sweep {
    pub common: Common,
    pub variants: PathBuf,
    pub only: Vec<String>,
    pub candidate: PathBuf,
    pub provisional: bool,
}

pub struct Arms {
    pub common: Common,
    pub arms: Vec<String>,
}

fn usage(message: String) -> NorthError {
    NorthError::Usage(format!("policy-replay: {message}"))
}

fn read(path: &Path) -> Result<String, NorthError> {
    fs::read_to_string(path).map_err(NorthError::Io)
}

fn load(path: &Path) -> Result<Value, NorthError> {
    serde_json::from_str(&read(path)?).map_err(NorthError::Json)
}

fn session_start_tokens(operator: &Path) -> Result<usize, NorthError> {
    let firn = std::env::var("NORTH_FIRN").unwrap_or_else(|_| format!("{}/code/nixos-config/main", std::env::var("HOME").unwrap_or_default()));
    let output = Command::new(std::env::current_exe().map_err(NorthError::Io)?)
        .args(["agent-config-check", "policy-gate", "measure", "--repo", &firn, "--operator"])
        .arg(operator)
        .output()
        .map_err(NorthError::Io)?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let pattern = Regex::new(r"(?m)^session start (\d+) tokens").expect("session start pattern compiles");
    pattern
        .captures(&stdout)
        .and_then(|caps| caps[1].parse().ok())
        .ok_or_else(|| usage(format!("no session start measurement from agent-config-check: {}", tail(&stdout, 300))))
}

fn startup_tokens(measured: usize, base: &str, text: &str) -> usize {
    let delta = (text.chars().count() as i64 - base.chars().count() as i64).div_euclid(4);
    (measured as i64 + delta).max(0) as usize
}

fn tail(text: &str, chars: usize) -> String {
    text.chars().skip(text.chars().count().saturating_sub(chars)).collect()
}

fn failed(rows: &[Value]) -> bool {
    rows.iter().any(|row| row.get("pass").is_none_or(Value::is_null))
}

fn run_rows(rows: &[Value]) -> Vec<RunRow> {
    rows.iter().map(run_row).collect()
}

fn scenario_policy(path: &Path, scenario: &Value, override_path: Option<&Path>) -> Result<PathBuf, NorthError> {
    if let Some(path) = override_path {
        return Ok(path.to_path_buf());
    }
    let here = fs::canonicalize(path).map_err(NorthError::Io)?;
    let parent = here.parent().unwrap_or(Path::new("/"));
    if let Some(policy) = scenario.get("policy").and_then(Value::as_str).filter(|policy| !policy.is_empty()) {
        let expanded = match policy.strip_prefix("~/") {
            Some(tail) => Path::new(&std::env::var("HOME").unwrap_or_default()).join(tail),
            None => PathBuf::from(policy),
        };
        return fs::canonicalize(parent.join(expanded)).map_err(NorthError::Io);
    }
    Ok(parent.parent().unwrap_or(Path::new("/")).join("AGENTS.md"))
}

pub fn arms(args: &Arms) -> Result<u8, NorthError> {
    install_signal_handlers();
    let out = args.common.out.as_deref().ok_or_else(|| usage("--arm and --out are required without a sweep".into()))?;
    no_fable(&[args.common.pick.model.as_deref()])?;
    let mut loaded: Vec<(Value, String)> = Vec::new();
    for path in &args.common.scenarios {
        let scenario = load(path)?;
        let policy = read(&scenario_policy(path, &scenario, args.common.policy.as_deref())?)?;
        for arm in &args.arms {
            policy_for_arm(&policy, &arm_minus(arm, &scenario)?)?;
        }
        loaded.push((scenario, policy));
    }
    let mut plan = Vec::new();
    for (scenario, policy) in &loaded {
        for arm in &args.arms {
            for trial in 1..=args.common.trials {
                plan.push(Item { scenario, arm: arm.clone(), trial, policy, pick: &args.common.pick });
            }
        }
    }
    let rows = run_plan(&plan, args.common.jobs, out)?;
    println!("{}", summary(&run_rows(&rows)));
    Ok(u8::from(failed(&rows)))
}

pub fn sweep(args: &Sweep) -> Result<u8, NorthError> {
    install_signal_handlers();
    let mut variants = Variant::parse_all(&read(&args.variants)?)?;
    if !args.only.is_empty() {
        let unknown: Vec<&String> = args.only.iter().filter(|name| variants.iter().all(|variant| &variant.name != *name)).collect();
        if !unknown.is_empty() {
            return Err(usage(format!("unknown variants {unknown:?}")));
        }
        variants.retain(|variant| args.only.contains(&variant.name));
    }
    let first = args.common.scenarios.first().ok_or_else(|| usage("the following arguments are required: scenarios".into()))?;
    let operator = fs::canonicalize(first).map_err(NorthError::Io)?.parent().and_then(Path::parent).unwrap_or(Path::new("/")).to_path_buf();
    let base = read(&args.common.policy.clone().unwrap_or_else(|| operator.join("AGENTS.md")))?;
    let measured = session_start_tokens(&operator)?;
    let mut policies: Vec<(String, String)> = Vec::new();
    for variant in &variants {
        policies.push((variant.name.clone(), variant_policy(&base, variant)?));
    }
    let startup: BTreeMap<&str, usize> = policies.iter().map(|(name, text)| (name.as_str(), startup_tokens(measured, &base, text))).collect();
    let scenarios: Vec<Value> = args.common.scenarios.iter().map(|path| load(path)).collect::<Result<_, _>>()?;
    let mut plan = Vec::new();
    for (name, text) in &policies {
        for scenario in &scenarios {
            for trial in 1..=args.common.trials {
                plan.push(Item { scenario, arm: format!("variant:{name}"), trial, policy: text, pick: &args.common.pick });
            }
        }
    }
    let out = args.common.out.clone().unwrap_or_else(|| operator.join("replay").join(format!("sweep-{}.jsonl", Timestamp::now().iso_date())));
    let stem = out.file_stem().and_then(|stem| stem.to_str()).unwrap_or("sweep").to_owned();
    let runs = run_plan(&plan, args.common.jobs, &state_dir().join(format!("{stem}-runs.jsonl")))?;
    let scored = run_rows(&runs);
    let rows: Vec<_> = policies
        .iter()
        .map(|(name, text)| {
            let arm = format!("variant:{name}");
            let own: Vec<RunRow> = scored.iter().filter(|run| run.arm == arm).cloned().collect();
            variant_row(name, text, startup[name.as_str()], &own)
        })
        .collect();
    let mut file = fs::OpenOptions::new().create(true).append(true).open(&out).map_err(NorthError::Io)?;
    for row in &rows {
        writeln!(file, "{}", serde_json::to_string(row)?).map_err(NorthError::Io)?;
    }
    println!("{}", summary(&scored));
    for row in &rows {
        let figure = |value: Option<f64>| value.map_or_else(|| "None".to_owned(), float_repr);
        println!(
            "{}: startup {} tokens, conformance {} (n={}), {} tokens/run, {}s",
            row.variant,
            row.startup_tokens,
            figure(row.conformance),
            row.n,
            row.runs_tokens.map_or_else(|| "None".to_owned(), |tokens| tokens.to_string()),
            float_repr(row.wall_s)
        );
    }
    let point = operating_point(&rows)?;
    write_candidate(&args.candidate, point, &out, args.provisional, &Timestamp::now().iso())?;
    let conformance = point.conformance.unwrap_or(0.0);
    let (low, high) = wilson(conformance, point.n);
    println!(
        "operating point: {} ({} tokens, conformance {} [{low:.4}, {high:.4}]) -> {}",
        point.variant,
        point.startup_tokens,
        float_repr(conformance),
        args.candidate.display()
    );
    Ok(u8::from(failed(&runs)))
}
