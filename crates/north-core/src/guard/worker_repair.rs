use super::worker_tier::{FACETS, REPOS};
use crate::hook::HookInput;
use crate::platform::pyjson::Value;
use regex::Regex;
use std::sync::LazyLock;

const ALIASES: &[(&str, &str, &str)] = &[
    ("Scope", "single-file", "one-file"),
    ("Scope", "one", "one-file"),
    ("Scope", "single-module", "module"),
    ("Scope", "single-repo", "cross-module"),
    ("Verify", "unit", "local-test"),
    ("Verify", "read-only", "none"),
    ("Verify", "report", "none"),
    ("Verify", "verdict", "none"),
    ("Verify", "reply", "none"),
    ("Verify", "review", "none"),
];
const SURFACES: &[(&str, &str)] = &[
    ("ts", "ts"),
    ("tsx", "ts"),
    ("rs", "rust"),
    ("lua", "lua"),
    ("nix", "nix"),
    ("sh", "shell"),
    ("md", "docs"),
];

static FACET_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([ \t]*)(Spec|Scope|Surface|Verify):([ \t]*)(.*)$").unwrap());
static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^\s.,;`'<>|)]*").unwrap());
static SEPARATORS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t_]+").unwrap());
static FILES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[ \t]*Files:(.*)$").unwrap());
static REPO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^[ \t]*Repo:").unwrap());

pub(super) fn note(changes: &[String]) -> String {
    format!(
        "worker-tier-guard corrected the worker brief before spawning: {}. Use the exact values next time.",
        changes.join("; ")
    )
}

pub(super) fn corrected(input: &HookInput, native: bool, prompt: &str) -> (Value, Vec<String>) {
    let mut updated = input.tool_input.clone();
    let mut changes = Vec::new();
    if !native && updated.get("run_in_background").is_some_and(|value| *value != Value::Bool(true)) {
        updated["run_in_background"] = Value::Bool(true);
        changes.push("set run_in_background: true".to_owned());
    }
    let fixed = corrected_prompt(prompt, &mut changes);
    if fixed != prompt {
        updated[if native { "message" } else { "prompt" }] = fixed.as_str().into();
    }
    (updated, changes)
}

fn corrected_prompt(prompt: &str, changes: &mut Vec<String>) -> String {
    let surface = files_surface(prompt);
    let mut repo = None;
    let mut lines = Vec::new();
    for line in prompt.split('\n') {
        let Some(found) = FACET_LINE.captures(line) else {
            lines.push(line.to_owned());
            continue;
        };
        let (indent, facet, gap, rest) = (&found[1], &found[2], &found[3], &found[4]);
        let allowed = FACETS.iter().find(|(name, _)| *name == facet).map_or(&[][..], |(_, values)| *values);
        let token = TOKEN.find(rest).map_or("", |token| token.as_str());
        let lower = token.to_lowercase();
        let whole = SEPARATORS.replace_all(rest.trim_end_matches(['.', ',', ';', ' ', '\t']), "-").to_lowercase();
        let replacement = if let Some((_, _, to)) = ALIASES.iter().find(|(name, from, _)| *name == facet && *from == lower) {
            Some((token.len(), (*to).to_owned()))
        } else if allowed.contains(&lower.as_str()) {
            (token != lower).then(|| (token.len(), lower.clone()))
        } else if allowed.contains(&whole.as_str()) {
            Some((rest.len(), whole))
        } else if facet == "Surface" && REPOS.contains(&lower.as_str()) {
            surface.map(|inferred| {
                repo = Some(lower.clone());
                (token.len(), inferred.to_owned())
            })
        } else {
            None
        };
        let Some((cut, value)) = replacement else {
            lines.push(line.to_owned());
            continue;
        };
        changes.push(format!("{facet}: {} -> {value}", rest[..cut].trim_end()));
        lines.push(format!("{indent}{facet}:{gap}{value}{}", &rest[cut..]));
        if facet == "Surface"
            && let Some(name) = repo.take()
            && !REPO.is_match(prompt)
        {
            changes.push(format!("moved the repository name to `Repo: {name}`, inferring Surface from the Files: line"));
            lines.push(format!("{indent}Repo: {name}"));
        }
    }
    let mut fixed = lines.join("\n");
    if crate::router::brief::landing_refusal(&fixed).is_some_and(|why| why.starts_with("Verify: native needs")) {
        let verify = Regex::new(r"(?m)^[ \t]*Verify:[^\n]*").unwrap();
        if let Some(end) = verify.find(&fixed).map(|found| found.end()) {
            fixed.insert_str(end, "\nLands-before: native");
            changes.push("added `Lands-before: native` (the standing rule for a Verify: native box)".to_owned());
        }
    }
    fixed
}

fn files_surface(prompt: &str) -> Option<&'static str> {
    let line = FILES.captures(prompt)?.get(1)?.as_str();
    let mut surfaces = line
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(|word| word.trim_matches(|c: char| "`'\"()[];:".contains(c)).trim_end_matches('.'))
        .filter(|word| word.contains('/') || word.rsplit_once('.').is_some_and(|(stem, _)| !stem.is_empty()))
        .map(|path| {
            if path.contains(".github/workflows") {
                return Some("workflow");
            }
            let (_, extension) = path.rsplit_once('.')?;
            SURFACES.iter().find(|(ext, _)| *ext == extension).map(|(_, surface)| *surface)
        });
    let first = surfaces.next()??;
    surfaces.all(|surface| surface == Some(first)).then_some(first)
}
