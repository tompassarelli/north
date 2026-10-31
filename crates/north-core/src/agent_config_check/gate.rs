use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use regex::Regex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::roots::Roots;
use crate::platform::trials::{Scenarios, TRIALS, Verdict, compare, flips};
use crate::platform::{process, pystats, toml_json};

const PLAYBOOK_LINES: usize = 500;
const SWEEP_DUE_SHARE: f64 = 0.15;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Mode {
    Budget,
    Measure,
    Session,
}

pub(super) struct Gate {
    pub(super) repo: PathBuf,
    pub(super) operator: PathBuf,
    pub(super) replay: PathBuf,
    pub(super) start: Box<dyn Fn(bool) -> Result<usize, String>>,
    pub(super) roots: Roots,
}

pub(super) struct Verdicts {
    pub(super) text: String,
    pub(super) passed: bool,
}

fn json_file(path: &Path) -> Result<Value, String> {
    fs::read_to_string(path)
        .map_err(|error| format!("{}: {error}", path.display()))
        .and_then(|body| serde_json::from_str(&body).map_err(|error| format!("{}: {error}", path.display())))
}

fn frontmatter(text: &str) -> Option<(String, usize)> {
    let pattern = Regex::new(r"(?s)\A---\n(.*?)\n---\n").unwrap();
    let found = pattern.captures(text)?;
    Some((format!("{}\n", &found[1]), found.get(0)?.end()))
}

fn description(front: &str) -> String {
    let pattern = fancy_regex::Regex::new(r"(?m)^description:[ \t]*(?:[>|]-?)?[ \t]*\n?((?:.*\n)*?)(?=^\S|\z)").unwrap();
    pattern
        .captures(front)
        .ok()
        .flatten()
        .and_then(|found| found.get(1).map(|text| text.as_str().split_whitespace().collect::<Vec<_>>().join(" ")))
        .unwrap_or_default()
}

fn scenarios_of(value: Option<&Value>) -> Scenarios {
    value
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(name, cell)| {
                    let cell = cell.as_array()?;
                    Some((name.clone(), [cell.first()?.as_u64()? as usize, cell.get(1)?.as_u64()? as usize]))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn covered(scenarios: &Scenarios, wanted: &BTreeSet<String>) -> usize {
    wanted.iter().filter(|name| scenarios.get(*name).is_some_and(|cell| cell[1] >= TRIALS)).count()
}

fn general(value: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    let decimals = (5 - value.abs().log10().floor() as i32).max(0) as usize;
    let text = format!("{value:.decimals$}");
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_owned() } else { text }
}

impl Gate {
    fn owner_path(&self, catalog: &Value, unit: &str) -> Option<PathBuf> {
        if let Some(owner) = catalog["registrations"][unit].get("owner").filter(|owner| !owner.is_null()) {
            let root = match owner.get("repo").and_then(Value::as_str) {
                None | Some("nixos-config") | Some("firn") => self.repo.clone(),
                Some(name) => self.roots.checkout(name),
            };
            return Some(root.join(owner["path"].as_str()?));
        }
        let found = Command::new("agents").args(["path", unit]).output().ok()?;
        found.status.success().then(|| PathBuf::from(String::from_utf8_lossy(&found.stdout).trim()))
    }

    fn instructions(&self, catalog: &Value, target: &str) -> Result<usize, String> {
        let mut text = String::new();
        for baseline in catalog["baselines"].as_array().into_iter().flatten() {
            if !baseline["targets"].as_array().is_some_and(|targets| targets.iter().any(|item| item == target)) {
                continue;
            }
            let owner = &baseline["owner"];
            let (name, path) = (owner["repo"].as_str().unwrap_or("None"), owner["path"].as_str().unwrap_or_default());
            let root = if name == "south" && path.starts_with("agents/") {
                self.operator.parent().map(Path::to_path_buf).unwrap_or_default()
            } else {
                self.roots.checkout(name)
            };
            let body = fs::read_to_string(root.join(path)).map_err(|error| format!("{}: {error}", root.join(path).display()))?;
            text.push_str(&format!("<!-- {name}:{path} -->\n{body}\n\n"));
        }
        Ok(text.chars().count())
    }

    fn startup_tokens(&self, catalog: &Value, claude_listing: usize) -> Result<(usize, String), String> {
        let orchestration = fs::read_to_string(self.operator.join("orchestration.toml")).map_err(|error| error.to_string())?;
        let orchestration = toml_json::parse(&orchestration).map_err(|error| error.to_string())?;
        let mut repos: Vec<(usize, String)> = orchestration["projects"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(name, _)| {
                let body = fs::read_to_string(self.roots.checkout(name).join("AGENTS.md")).ok()?;
                Some((body.chars().count(), name.clone()))
            })
            .collect();
        repos.sort_by(|left, right| right.cmp(left));
        let mut agent_chars = 0;
        let agents = self.roots.home.join(".claude/agents");
        for entry in fs::read_dir(&agents).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "md") {
                continue;
            }
            let body = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            let Some((front, _)) = frontmatter(&body) else { continue };
            for field in ["name", "description"] {
                let value = Regex::new(&format!(r"(?m)^{field}:\s*(.*)$")).unwrap();
                agent_chars += value.captures(front.trim_end_matches('\n')).map_or(0, |found| found[1].chars().count());
            }
        }
        let start = (self.start)(true)?;
        let live = (self.start)(false)?;
        let parts = [
            ("policy".to_owned(), self.instructions(catalog, "shared")?),
            ("code".to_owned(), self.instructions(catalog, "code")?),
            ("skills".to_owned(), claude_listing),
            ("agents".to_owned(), agent_chars),
            ("SessionStart fleet".to_owned(), start),
        ];
        let detail = parts.iter().map(|(name, chars)| format!("{name} {}", chars / 4)).collect::<Vec<_>>().join(", ");
        let project = if repos.is_empty() {
            "none".to_owned()
        } else {
            repos.iter().map(|(chars, name)| format!("{name} {}", chars / 4)).collect::<Vec<_>>().join(", ")
        };
        let total = parts.iter().map(|(_, chars)| chars).sum::<usize>() / 4;
        Ok((total, format!("chars/4: {detail} (fixture state; live SessionStart {}, not gated); harness-bundled listing not counted; project AGENTS.md, not gated: {project}", live / 4)))
    }

    fn sweep_row(&self, sha: &str, wanted: Option<&BTreeSet<String>>) -> Option<Value> {
        let mut files: Vec<PathBuf> = Vec::new();
        for directory in [self.operator.join("replay"), self.replay.clone()] {
            for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("sweep-") && name.ends_with(".jsonl") {
                    files.push(entry.path());
                }
            }
        }
        files.sort_by_key(|path| path.file_name().map(|name| name.to_owned()));
        let (mut newest, mut full) = (None, None);
        for path in files {
            for line in fs::read_to_string(&path).unwrap_or_default().lines() {
                let Ok(row) = serde_json::from_str::<Value>(line) else { continue };
                if row.get("policy_sha256").and_then(Value::as_str) != Some(sha) {
                    continue;
                }
                let scenarios = scenarios_of(row.get("scenarios"));
                let need: BTreeSet<String> = wanted.cloned().unwrap_or_else(|| scenarios.keys().cloned().collect());
                if !need.is_empty() && covered(&scenarios, &need) == need.len() {
                    full = Some(row.clone());
                }
                newest = Some(row);
            }
        }
        full.or(newest)
    }

    fn scenario_ids(&self) -> BTreeSet<String> {
        let mut ids = BTreeSet::new();
        for entry in fs::read_dir(self.operator.join("replay")).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            let id = fs::read_to_string(&path)
                .ok()
                .and_then(|body| serde_json::from_str::<Value>(&body).ok())
                .and_then(|value| value.as_object().map(|object| object.get("id").and_then(Value::as_str).filter(|id| !id.is_empty()).map(str::to_owned)))
                .flatten();
            ids.insert(id.unwrap_or(stem));
        }
        ids
    }

    fn regression(&self, point: Option<&Value>, sha: &str, tokens: usize) -> Result<String, String> {
        let Some(point) = point else {
            return Err("agents sync: no operating point; run the sweep and promote one".into());
        };
        let point_row = point.get("policy_sha256").and_then(Value::as_str).and_then(|point_sha| self.sweep_row(point_sha, None)).unwrap_or(Value::Null);
        let mut wanted = self.scenario_ids();
        let point_scenarios = scenarios_of(point_row.get("scenarios"));
        if !point_scenarios.is_empty() {
            wanted.retain(|name| point_scenarios.contains_key(name));
        }
        let Some(row) = self.sweep_row(sha, Some(&wanted)) else {
            return Err(format!(
                "agents sync: the live policy has no sweep row in {} or {}; run policy-replay.py --sweep with it as a variant",
                self.operator.join("replay").display(),
                self.replay.display()
            ));
        };
        if row.get("model") != point.get("model") {
            return Err(format!(
                "agents sync: the live policy's sweep row ran on {}, the operating point on {}; rerun the sweep on one model",
                shown(row.get("model")),
                shown(point.get("model"))
            ));
        }
        let row_scenarios = scenarios_of(row.get("scenarios"));
        let count = covered(&row_scenarios, &wanted);
        if count < wanted.len() {
            return Err(format!("agents sync: sweep row for {} covers {count} of {} scenarios", &sha[..8.min(sha.len())], wanted.len()));
        }
        let flipped = flips(&point_scenarios, &row_scenarios);
        if !flipped.is_empty() {
            return Err(format!(
                "agents sync: scenario {} flipped to 0 passes where the operating point passed every trial; confirm at 10 trials per arm",
                flipped.join(", ")
            ));
        }
        judge(point, &row, tokens)
    }

    fn skill_budget(&self, catalog: &Value) -> Result<Budget, String> {
        let mut budget = Budget::default();
        let kind_domain = Regex::new(r"(?m)^metadata:\n(?:[ \t]+.*\n)*?[ \t]+kind:[ \t]*domain[ \t]*$").unwrap();
        let agents_line = Regex::new(r"(?m)^agents:\s*\[(.*)\]").unwrap();
        for (unit, entry) in catalog["activation"].as_object().into_iter().flatten() {
            if !distributes_shared_skill(entry) {
                continue;
            }
            let Some(skill) = self.owner_path(catalog, unit).filter(|path| path.is_file()) else {
                budget.missing += 1;
                continue;
            };
            let text = fs::read_to_string(&skill).map_err(|error| format!("{}: {error}", skill.display()))?;
            let Some((front, body_start)) = frontmatter(&text) else {
                budget.errors.push(format!("{unit}: SKILL.md has no frontmatter"));
                continue;
            };
            let lines = text[body_start..].lines().count();
            budget.total += lines;
            if lines > PLAYBOOK_LINES && !kind_domain.is_match(&front) {
                budget.errors.push(format!(
                    "{unit}: playbook body is {lines} lines; policy keeps each playbook body at most {PLAYBOOK_LINES} (set metadata.kind: domain for a domain skill)"
                ));
            }
            let agents = agents_line.captures(&front).map(|found| found[1].to_owned());
            let size = unit.chars().count() + description(&front).chars().count();
            if agents.as_ref().is_none_or(|agents| agents.contains("claude")) {
                budget.claude_listing += size + 4;
            }
            if agents.as_ref().is_none_or(|agents| agents.contains("codex")) {
                budget.listing += size;
            }
        }
        Ok(budget)
    }

    fn session(&self, tokens: usize, detail: String, lines: &mut Vec<String>, errors: &mut Vec<String>) -> Result<(), String> {
        let sha = hex(&Sha256::digest(fs::read(self.operator.join("AGENTS.md")).map_err(|error| error.to_string())?));
        let point = match fs::read_to_string(self.operator.join("operating-point.toml")) {
            Ok(text) => Some(toml_json::parse(&text).map_err(|error| error.to_string())?),
            Err(_) => None,
        };
        match self.regression(point.as_ref(), &sha, tokens) {
            Ok(line) => lines.push(line),
            Err(failure) => errors.push(failure),
        }
        lines.push(detail);
        if let Some(point) = &point {
            let startup = point["startup_tokens"].as_i64().unwrap_or(0);
            if ((tokens as i64 - startup).abs() as f64) > SWEEP_DUE_SHARE * startup as f64 {
                lines.push(format!("sweep due: startup tokens {tokens} differ from the operating point's {startup} by more than 15%"));
            }
        }
        Ok(())
    }

    pub(super) fn run(&self, mode: Mode) -> Result<Verdicts, String> {
        let catalog = json_file(&self.operator.join("catalog-config.json"))?;
        let mut budget = self.skill_budget(&catalog)?;
        let mut lines: Vec<String> = Vec::new();
        if mode != Mode::Budget {
            let (tokens, detail) = self.startup_tokens(&catalog, budget.claude_listing)?;
            if mode == Mode::Measure {
                lines.push(format!("session start {tokens} tokens"));
                lines.push(detail);
            } else {
                self.session(tokens, detail, &mut lines, &mut budget.errors)?;
            }
        }
        lines.push(if budget.errors.is_empty() { budget.summary() } else { budget.errors.join("\n") });
        Ok(Verdicts { text: lines.join("\n"), passed: budget.errors.is_empty() })
    }
}

#[derive(Default)]
struct Budget {
    errors: Vec<String>,
    total: usize,
    listing: usize,
    claude_listing: usize,
    missing: usize,
}

impl Budget {
    fn summary(&self) -> String {
        let mut summary = format!("{} SKILL body lines · {} Codex listing chars", self.total, self.listing);
        if self.missing > 0 {
            summary.push_str(&format!(" · {} unreadable sources skipped", self.missing));
        }
        summary
    }
}

fn distributes_shared_skill(entry: &Value) -> bool {
    entry["distributions"].as_array().into_iter().flatten().any(|distribution| {
        distribution["type"] == "skill" && distribution["targets"].as_array().is_some_and(|targets| targets.iter().any(|target| target == "shared"))
    })
}

fn shown(value: Option<&Value>) -> String {
    value.map_or("None".to_owned(), |value| value.as_str().map_or(value.to_string(), str::to_owned))
}

fn judge(point: &Value, row: &Value, tokens: usize) -> Result<String, String> {
    let old = (point["conformance"].as_f64().unwrap_or(0.0), point["n"].as_u64().unwrap_or(0) as usize);
    let new = (row["conformance"].as_f64().unwrap_or(0.0), row["n"].as_u64().unwrap_or(0) as usize);
    let verdict = compare(old, new);
    if verdict == Verdict::Drop {
        return Err(format!("agents sync: replay conformance dropped {}→{}", general(old.0), general(new.0)));
    }
    let startup = point["startup_tokens"].as_u64().unwrap_or(0) as usize;
    if tokens > startup && verdict != Verdict::Gain {
        return Err(format!(
            "agents sync: startup tokens grew {startup}→{tokens} with no measured conformance gain; run policy-replay.py --sweep"
        ));
    }
    let (low, high) = pystats::wilson(old.0, old.1);
    Ok(format!(
        "session start {tokens} tokens (operating point {} {startup} at {}, conformance {} [{low:.3}, {high:.3}] n={})",
        point.get("variant").and_then(Value::as_str).unwrap_or("?"),
        shown(point.get("date")),
        general(old.0),
        old.1
    ))
}

pub(super) fn north_session_start(north: OsString, home: PathBuf) -> Box<dyn Fn(bool) -> Result<usize, String>> {
    Box::new(move |fixture| {
        let payload = json!({"hook_event_name": "SessionStart", "source": "startup", "session_id": "budget", "cwd": home});
        let mut command = Command::new(&north);
        command.args(["hook", "session-start"]).env_remove("AGENT_NO_AUTHORING_HOOKS").env("CLAUDE_CODE_ENTRYPOINT", "cli");
        if fixture {
            command.env("LANE_GC_UNLANDED", "/dev/null");
        }
        let output = process::run_command_within(command, Some(payload.to_string().as_bytes()), Duration::from_secs(10))
            .ok_or("north hook session-start did not finish within 10 seconds")?;
        Ok(String::from_utf8_lossy(&output.stdout).chars().count())
    })
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
