use regex::Regex;
use serde_json::{Value, json};

use super::json::{num, opt};
use super::policy::{high_confidence_runs, learned_minimum_runs, owner_prefix, resource_class, sizing_runs};

#[derive(Clone, Debug, PartialEq)]
pub struct TaskSignature {
    pub family: String,
    pub repo: Option<String>,
    pub package: Option<String>,
    pub class: String,
    pub exact: bool,
}

impl TaskSignature {
    fn json(&self) -> Value {
        json!({ "family": self.family, "repo": self.repo, "package": self.package, "class": self.class, "exact": self.exact })
    }
}

/// A recorded run as the capacity database holds it.
#[derive(Clone, Debug)]
pub struct CapacityRun {
    pub at: String,
    pub signature: TaskSignature,
    pub owner: Option<String>,
    pub shape: Option<String>,
    pub mean_cores: Option<f64>,
    pub p90_cores: Option<f64>,
    pub peak_memory_mib: Option<f64>,
    pub io_read_bytes: Option<f64>,
    pub io_write_bytes: Option<f64>,
    pub disk_busy_share: Option<f64>,
    pub wall_seconds: Option<f64>,
    pub exit_code: Option<f64>,
}

fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

const FIRN_VERBS: [&str; 3] = ["build", "validate", "rebuild"];

fn tool_family(words: &[String], program: &str) -> Option<String> {
    let word = |index: usize| words.get(index).map_or("", String::as_str);
    let wisp_render = word(1) == "wisp" && (word(2) == "view" || (word(2) == "headless" && words.iter().any(|word| word == "--render")));
    Some(match program {
        "cargo" if matches!(word(1), "build" | "test") => format!("cargo {}", word(1)),
        "bun" if word(1) == "test" => "bun test".to_owned(),
        "bun" if word(1) == "wisp" && word(2) == "map" && word(3) == "build" => "bun wisp map build".to_owned(),
        "bun" if wisp_render => "wisp render".to_owned(),
        "wisp-render" => "wisp render".to_owned(),
        "codex" | "claude" => format!("{program} agent"),
        _ => return None,
    })
}

fn family(words: &[String], program: &str, repo: Option<&str>, kind: &str) -> String {
    if let Some(family) = tool_family(words, program) {
        return family;
    }
    if kind == "native" || matches!(program, "wine" | "wine64" | "proton" | "steam-run") || program.to_ascii_lowercase().ends_with(".exe") {
        return "native client".to_owned();
    }
    if program == "nixos-rebuild" {
        return "firn rebuild".to_owned();
    }
    match words.iter().find(|word| FIRN_VERBS.contains(&word.as_str())) {
        Some(verb) if matches!(repo, Some("firn" | "nixos-config")) || program == "firn" => format!("firn {verb}"),
        _ => "other".to_owned(),
    }
}

fn target(words: &[String], program: &str) -> Option<String> {
    let mut target: Option<String> = None;
    for (index, word) in words.iter().enumerate().skip(1) {
        if program == "cargo" && matches!(word.as_str(), "-p" | "--package" | "--manifest-path") {
            target = words.get(index + 1).cloned();
        } else if let Some(rest) = word.strip_prefix("--package=").or_else(|| word.strip_prefix("--manifest-path=")).filter(|_| program == "cargo") {
            target = Some(rest.to_owned());
        } else if program == "cargo" && word.starts_with("-p") && word.len() > 2 {
            target = Some(word[2..].to_owned());
        } else if !matches!(program, "codex" | "claude") && !word.starts_with('-') && !word.chars().any(char::is_whitespace) && (word.contains('/') || [".ts", ".js", ".mjs", ".toml"].iter().any(|suffix| word.ends_with(suffix))) && target.is_none() {
            target = Some(word.strip_prefix("./").unwrap_or(word).to_owned());
        }
    }
    target
}

pub fn task_signature(argv: &[String], cwd: Option<&str>, kind: &str, exact: bool) -> TaskSignature {
    let mut words = argv.to_vec();
    if words.first().map(|word| basename(word)) == Some("nix") && words.get(1).map(String::as_str) == Some("develop") && let Some(marker) = words.iter().position(|word| word == "-c") {
        words = words[marker + 1..].to_vec();
    }
    if words.first().map(|word| basename(word)) == Some("env") {
        words.remove(0);
        while words.first().is_some_and(|word| word.contains('=')) {
            words.remove(0);
        }
    }
    let program = words.first().map_or("", |word| basename(word)).to_owned();
    static REPO: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"^(?:~|/home/[^/]+)/code/([^/]+)(?:/|$)").expect("repo pattern"));
    let repo = cwd.and_then(|cwd| REPO.captures(cwd)).map(|captures| captures[1].to_owned());
    TaskSignature {
        family: family(&words, &program, repo.as_deref(), kind),
        package: if exact { target(&words, &program) } else { None },
        repo,
        class: kind.to_owned(),
        exact,
    }
}

#[derive(Clone, Debug, Default)]
pub struct CapacityQuery {
    pub owner: Option<String>,
    pub shape: Option<String>,
}

/// The ceil(n × share)-th smallest value, or None for no values.
fn quantile(values: &[f64], share: f64) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = (sorted.len() as f64 * share).ceil() as usize;
    sorted.get(index.checked_sub(1)?).copied()
}

fn same_exact(signature: &TaskSignature, run: &CapacityRun) -> bool {
    signature.exact && run.signature.exact && run.signature.family == signature.family && run.signature.repo == signature.repo && run.signature.package == signature.package && run.signature.class == signature.class
}

fn duration(signature: &TaskSignature, history: &[CapacityRun]) -> Value {
    let mut runs: Vec<&CapacityRun> = history
        .iter()
        .filter(|run| same_exact(signature, run) && run.wall_seconds.is_some_and(|seconds| seconds.is_finite() && seconds > 0.0) && run.exit_code.unwrap_or(0.0) == 0.0)
        .collect();
    runs.sort_by(|left, right| left.at.cmp(&right.at));
    let seconds: Vec<f64> = runs[runs.len().saturating_sub(sizing_runs())..].iter().filter_map(|run| run.wall_seconds).collect();
    let minimum = learned_minimum_runs();
    if seconds.len() >= minimum {
        json!({ "p75Seconds": opt(quantile(&seconds, 0.75)), "sampleCount": seconds.len(), "basis": "exact" })
    } else {
        json!({ "p75Seconds": null, "sampleCount": seconds.len(), "basis": format!("uncalibrated: {} exact runs < {minimum}", seconds.len()) })
    }
}

const DISK_BASIS: &str = "host maximum physical whole-disk busy fraction";

fn conservative_class(family: &str) -> Option<&'static str> {
    match family {
        "native client" => Some("native"),
        "cargo build" | "cargo test" | "bun test" | "wisp render" | "firn rebuild" => Some("heavy"),
        _ => None,
    }
}

type Level<'a> = (&'static str, Box<dyn Fn(&CapacityRun) -> bool + 'a>);

fn levels<'a>(signature: &'a TaskSignature, query: &'a CapacityQuery) -> [Level<'a>; 5] {
    let owner_shape = move |run: &CapacityRun| query.owner.is_some() && query.shape.is_some() && run.shape == query.shape;
    [
        ("exact", Box::new(move |run| same_exact(signature, run))),
        ("owner-shape", Box::new(move |run| owner_shape(run) && run.owner == query.owner)),
        ("owner-prefix-shape-class", Box::new(move |run| {
            owner_shape(run) && run.signature.class == signature.class && run.owner.as_deref().zip(query.owner.as_deref()).is_some_and(|(left, right)| owner_prefix(left) == owner_prefix(right))
        })),
        ("family-repo", Box::new(move |run| signature.repo.is_some() && run.signature.family == signature.family && run.signature.repo == signature.repo)),
        ("family", Box::new(move |run| signature.family != "other" && run.signature.family == signature.family)),
    ]
}

fn distribution(values: &[f64]) -> Value {
    json!({ "p50": opt(quantile(values, 0.5)), "p90": opt(quantile(values, 0.9)) })
}

fn measured(signature: &TaskSignature, level: &str, runs: &[&CapacityRun], duration: Value) -> Value {
    let values = |pick: fn(&CapacityRun) -> Option<f64>| runs.iter().filter_map(|run| pick(run)).filter(|value| value.is_finite()).collect::<Vec<f64>>();
    let confidence = if level == "exact" && runs.len() >= high_confidence_runs() { "high" } else { "medium" };
    json!({
        "signature": signature.json(), "class": signature.class, "sampleCount": runs.len(), "matchLevel": level, "confidence": confidence,
        "cores": { "p50": opt(quantile(&values(|run| run.mean_cores), 0.5)), "p90": opt(quantile(&values(|run| run.p90_cores), 0.9)) },
        "peakMemoryMiB": distribution(&values(|run| run.peak_memory_mib)), "ioReadBytes": distribution(&values(|run| run.io_read_bytes)),
        "ioWriteBytes": distribution(&values(|run| run.io_write_bytes)), "diskBusyShare": distribution(&values(|run| run.disk_busy_share)),
        "diskBusyBasis": DISK_BASIS, "ioSampleCount": values(|run| run.io_read_bytes).len(), "duration": duration,
    })
}

/// What a run of this signature is expected to use: its closest recorded runs, else its class's declaration.
pub fn estimate_capacity(signature: &TaskSignature, history: &[CapacityRun], cores: f64, query: &CapacityQuery) -> Value {
    let mut valid: Vec<&CapacityRun> = history.iter().filter(|run| run.p90_cores.is_some_and(|cores| cores.is_finite() && cores >= 0.0)).collect();
    valid.sort_by(|left, right| left.at.cmp(&right.at));
    let conservative = conservative_class(&signature.family);
    let unseen_expensive = conservative.is_some()
        && signature.exact
        && !valid.iter().any(|run| run.signature.exact && run.signature.family == signature.family && run.signature.repo == signature.repo && run.signature.package == signature.package);
    let duration = duration(signature, history);
    for (level, matches) in levels(signature, query) {
        if unseen_expensive {
            break;
        }
        let matching: Vec<&CapacityRun> = valid.iter().copied().filter(|run| matches(run)).collect();
        let runs = &matching[matching.len().saturating_sub(sizing_runs())..];
        if runs.len() >= learned_minimum_runs() {
            return measured(signature, level, runs, duration);
        }
    }
    let kind = conservative.unwrap_or(&signature.class);
    let declared = resource_class(kind, cores).unwrap_or(super::policy::Resources { cpus: f64::NAN, memory_mib: f64::NAN });
    let unknown = json!({ "p50": null, "p90": null });
    json!({
        "signature": signature.json(), "class": kind, "sampleCount": 0, "matchLevel": "default", "confidence": "low",
        "cores": { "p50": num(declared.cpus), "p90": num(declared.cpus) },
        "peakMemoryMiB": { "p50": num(declared.memory_mib), "p90": num(declared.memory_mib) },
        "ioReadBytes": unknown, "ioWriteBytes": unknown, "diskBusyShare": unknown,
        "diskBusyBasis": DISK_BASIS, "ioSampleCount": 0, "duration": duration,
    })
}
