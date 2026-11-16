use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use regex::Regex;
use serde_json::{Value, json};

use super::roots::Roots;
use north_base::platform::{host, process, toml_json};

const PLAYBOOK_LINES: usize = 500;
const SKILL_BYTES: usize = 6144;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Mode {
    Budget,
    Measure,
}

pub(super) struct Gate {
    pub(super) repo: PathBuf,
    pub(super) operator: PathBuf,
    pub(super) start: Box<dyn Fn(bool) -> Result<usize, String>>,
    pub(super) roots: Roots,
}

pub(super) struct Verdicts {
    pub(super) text: String,
    pub(super) passed: bool,
}

pub(super) fn frontmatter(text: &str) -> Option<(String, usize)> {
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

    fn skill_budget(&self, catalog: &Value) -> Result<Budget, String> {
        let mut budget = Budget::default();
        for (unit, entry) in catalog["activation"].as_object().into_iter().flatten() {
            if !distributes_shared_skill(entry) {
                continue;
            }
            let Some(skill) = self.owner_path(catalog, unit).filter(|path| path.is_file()) else {
                budget.missing += 1;
                continue;
            };
            let text = fs::read_to_string(&skill).map_err(|error| format!("{}: {error}", skill.display()))?;
            budget.measure(unit, &text);
        }
        Ok(budget)
    }

    pub(super) fn startup_total(&self) -> Result<usize, String> {
        let catalog = crate::agent_catalog::owner_modules::read_resolved_catalog(&self.operator.join("catalog-config.json")).map_err(|error| error.to_string())?;
        let budget = self.skill_budget(&catalog)?;
        Ok(self.startup_tokens(&catalog, budget.claude_listing)?.0)
    }

    pub(super) fn run(&self, mode: Mode) -> Result<Verdicts, String> {
        let catalog = crate::agent_catalog::owner_modules::read_resolved_catalog(
            &self.operator.join("catalog-config.json"),
        )
        .map_err(|error| error.to_string())?;
        let budget = self.skill_budget(&catalog)?;
        let mut lines: Vec<String> = Vec::new();
        if mode != Mode::Budget {
            let (tokens, detail) = self.startup_tokens(&catalog, budget.claude_listing)?;
            lines.push(format!("session start {tokens} tokens"));
            lines.push(detail);
        }
        lines.push(if budget.errors.is_empty() { budget.summary() } else { budget.errors.join("\n") });
        Ok(Verdicts { text: lines.join("\n"), passed: budget.errors.is_empty() })
    }
}

#[derive(Default)]
pub(super) struct Budget {
    pub(super) errors: Vec<String>,
    total: usize,
    listing: usize,
    claude_listing: usize,
    missing: usize,
}

impl Budget {
    pub(super) fn measure(&mut self, unit: &str, text: &str) {
        let kind_domain = Regex::new(r"(?m)^metadata:\n(?:[ \t]+.*\n)*?[ \t]+kind:[ \t]*domain[ \t]*$").unwrap();
        let agents_line = Regex::new(r"(?m)^agents:\s*\[(.*)\]").unwrap();
        let Some((front, body_start)) = frontmatter(text) else {
            self.errors.push(format!("{unit}: SKILL.md has no frontmatter"));
            return;
        };
        let lines = text[body_start..].lines().count();
        self.total += lines;
        if lines > PLAYBOOK_LINES && !kind_domain.is_match(&front) {
            self.errors.push(format!(
                "{unit}: playbook body is {lines} lines; policy keeps each playbook body at most {PLAYBOOK_LINES} (set metadata.kind: domain for a domain skill)"
            ));
        }
        if text.len() > SKILL_BYTES {
            self.errors.push(format!("{unit}: SKILL.md is {} bytes; policy keeps each SKILL.md at most {SKILL_BYTES}, as a router with read-X-when-Y references", text.len()));
        }
        if Regex::new(r"(?m)^paths:").unwrap().is_match(&front) {
            self.errors.push(format!("{unit}: SKILL.md sets paths:, which hides the skill from Claude's listing until a matching file is touched and never loads its body; harness/file-skills.toml and the file_skill hook load language skills"));
        }
        let agents = agents_line.captures(&front).map(|found| found[1].to_owned());
        let size = unit.chars().count() + description(&front).chars().count();
        if agents.as_ref().is_none_or(|agents| agents.contains("claude")) {
            self.claude_listing += size + 4;
        }
        if agents.as_ref().is_none_or(|agents| agents.contains("codex")) {
            self.listing += size;
        }
    }

    pub(super) fn summary(&self) -> String {
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

const START_DEADLINE: Duration = Duration::from_secs(10);
const LEASE_WAIT_SECONDS: u64 = 600;

pub fn north_session_start(north: OsString, home: PathBuf) -> Box<dyn Fn(bool) -> Result<usize, String>> {
    session_start_probe(north, home, START_DEADLINE, host::contention)
}

pub(super) fn session_start_probe(north: OsString, home: PathBuf, deadline: Duration, contention: fn() -> Option<f64>) -> Box<dyn Fn(bool) -> Result<usize, String>> {
    Box::new(move |fixture| {
        let mut payload = json!({"hook_event_name": "SessionStart", "source": "startup", "session_id": "budget", "cwd": home});
        if fixture {
            payload["agent_type"] = json!("worker");
        }
        let payload = payload.to_string();
        let seconds = deadline.as_secs();
        let hook = |leased: bool| {
            let mut command = Command::new(&north);
            if leased {
                command.args(["capacity", "host", "run", "--class", "moderate", "--owner", "agent-config-check", "--timeout-seconds", &LEASE_WAIT_SECONDS.to_string(), "--", "timeout", &seconds.to_string()]).arg(&north);
            }
            command.args(["hook", "session-start"]).env_remove("AGENT_NO_AUTHORING_HOOKS").env("CLAUDE_CODE_ENTRYPOINT", "cli");
            if fixture {
                command.env("LANE_GC_UNLANDED", "/dev/null");
            }
            command
        };
        let stalled = match process::try_run_command_traced_within(hook(false), Some(payload.as_bytes()), deadline)
            .map_err(|error| format!("north hook session-start did not run: {}: {error}", program(&north)))?
        {
            Ok(output) => return Ok(String::from_utf8_lossy(&output.stdout).chars().count()),
            Err(stalled) => stalled,
        };
        let Some(load) = contention().filter(|load| *load > 1.0) else {
            return Err(format!("north hook session-start did not finish within {seconds} seconds; at the limit: {stalled}"));
        };
        eprintln!("agent-config-check: host saturated at {load:.2} load per CPU, so north hook session-start missed its {seconds} s deadline; retrying under a capacity lease");
        let output = process::try_run_command_traced_within(hook(true), Some(payload.as_bytes()), Duration::from_secs(LEASE_WAIT_SECONDS) + deadline)
            .map_err(|error| format!("north hook session-start did not run under a capacity lease: {}: {error}", program(&north)))?
            .map_err(|stalled| format!("host saturated at {load:.2} load per CPU; no capacity lease admitted north hook session-start within {LEASE_WAIT_SECONDS} seconds; at the limit: {stalled}"))?;
        match output.status.code() {
            Some(0) => {}
            Some(124) => return Err(format!("host saturated at {load:.2} load per CPU; under a capacity lease north hook session-start still did not finish within {seconds} seconds")),
            _ => return Err(format!("host saturated at {load:.2} load per CPU; under a capacity lease north hook session-start failed ({}): {}", output.status, String::from_utf8_lossy(&output.stderr).trim())),
        }
        Ok(String::from_utf8_lossy(&output.stdout).chars().count())
    })
}

fn program(north: &OsStr) -> String {
    let path = Path::new(north);
    format!("{} ({})", path.display(), match (path.is_absolute(), path.exists()) {
        (false, _) => "looked up on PATH",
        (true, true) => "present",
        (true, false) => "missing",
    })
}
