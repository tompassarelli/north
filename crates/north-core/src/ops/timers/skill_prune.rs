use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use regex::Regex;
use serde_json::Value;
use toml_edit::{DocumentMut, Value as TomlValue};

use super::{child_failure, date, env_or, home, north_program, text};
use crate::platform::process;

const SESSION_LIMIT: Duration = Duration::from_secs(5400);

struct Paths {
    handoffs: std::path::PathBuf,
    operator: std::path::PathBuf,
}

pub fn run(arguments: &[String]) -> u8 {
    let paths = Paths {
        handoffs: env_or("AGENTS_HANDOFFS", || home().join(".local/state/agents/handoffs")),
        operator: env_or("AGENTS_OPERATOR_DIR", || home().join("code/south/main/agents")),
    };
    match arguments.first().map(String::as_str) {
        Some("-h" | "--help") => print_usage(&paths),
        Some("--measure") => match measure(&paths.operator) {
            Ok(table) => println!("{table}"),
            Err(error) => return fail(&error),
        },
        _ => return prune(&paths).map_or_else(|error| fail(&error), |report| {
            println!("{}", report.display());
            0
        }),
    }
    0
}

fn fail(error: &str) -> u8 {
    eprintln!("skill-prune: {error}");
    1
}

fn print_usage(paths: &Paths) {
    let report = paths.handoffs.join("skill-prune.md");
    let lessons = paths.handoffs.join("codex-lessons.md");
    println!("usage: skill-prune [--measure]");
    println!("Weekly: one Claude Opus medium session prunes the shared SKILL.md bodies toward the 60-line playbook target and");
    println!("shrinks duplicated or stale rules, folds {}, lands each repository through safe-push, and writes the", lessons.display());
    println!("counts to {}.", report.display());
    println!("--measure prints every shared skill's body lines (domain skills marked) and the total beside the operating point");
    println!("in {} (informational, no cap).", paths.operator.join("operating-point.toml").display());
}

fn skill_text(unit: &str) -> String {
    let found = Command::new("agents").args(["path", unit]).output();
    let Some(found) = found.ok().filter(|output| output.status.success()) else {
        return String::new();
    };
    fs::read_to_string(text(&found).trim()).unwrap_or_default()
}

fn is_shared_skill(entry: &Value) -> bool {
    entry["distributions"].as_array().is_some_and(|all| {
        all.iter().any(|dist| dist["type"] == "skill" && dist["targets"].as_array().is_some_and(|t| t.iter().any(|target| target == "shared")))
    })
}

pub(super) fn body_row(unit: &str, skill: &str) -> (usize, String, &'static str) {
    let frontmatter = Regex::new(r"(?s)\A---\n(.*?)\n---\n").expect("frontmatter pattern");
    let Some(found) = frontmatter.captures(skill) else {
        return (0, unit.to_owned(), "unreadable");
    };
    let lines = skill[found.get(0).map_or(0, |whole| whole.end())..].lines().count();
    let domain = Regex::new(r"(?m)^metadata:\n(?:[ \t]+.*\n)*?[ \t]+kind:[ \t]*domain[ \t]*$").expect("domain pattern");
    let mark = if domain.is_match(&format!("{}\n", &found[1])) {
        "domain"
    } else if lines > 60 {
        "over 60"
    } else {
        ""
    };
    (lines, unit.to_owned(), mark)
}

pub(super) fn render_point(point: Option<&str>, total: usize) -> String {
    let Some(document) = point.and_then(|raw| raw.parse::<DocumentMut>().ok()) else {
        return format!("total {total} (no operating point yet)");
    };
    let field = |name: &str| document.get(name).and_then(|item| item.as_value()).map(render_value);
    let skill_lines = field("skill_lines").map(|value| format!("skill lines {value}, ")).unwrap_or_default();
    format!(
        "total {total} (operating point {}: {skill_lines}startup tokens {}, conformance {})",
        field("date").unwrap_or_default(),
        field("startup_tokens").unwrap_or_default(),
        field("conformance").unwrap_or_default()
    )
}

fn render_value(value: &TomlValue) -> String {
    match value {
        TomlValue::String(text) => text.value().clone(),
        TomlValue::Integer(number) => number.value().to_string(),
        TomlValue::Float(number) => format!("{:?}", number.value()),
        TomlValue::Boolean(flag) => if *flag.value() { "True" } else { "False" }.to_owned(),
        TomlValue::Datetime(stamp) => stamp.value().to_string(),
        _ => String::new(),
    }
}

fn measure(operator: &Path) -> Result<String, String> {
    let config = operator.join("catalog-config.json");
    let catalog: Value = serde_json::from_slice(&fs::read(&config).map_err(|error| format!("{}: {error}", config.display()))?)
        .map_err(|error| format!("{}: {error}", config.display()))?;
    let mut rows = Vec::new();
    let mut total = 0;
    for (unit, _) in catalog["activation"].as_object().into_iter().flatten().filter(|(_, entry)| is_shared_skill(entry)) {
        let row = body_row(unit, &skill_text(unit));
        if row.2 != "unreadable" {
            total += row.0;
        }
        rows.push(row);
    }
    rows.sort();
    rows.reverse();
    let mut table: Vec<String> = rows.iter().map(|(lines, unit, mark)| format!("{lines:5} {unit} {mark}").trim_end().to_owned()).collect();
    let point = fs::read_to_string(operator.join("operating-point.toml")).ok();
    table.push(render_point(point.as_deref(), total));
    Ok(table.join("\n"))
}

fn brief(handoffs: &Path, lessons: &Path) -> String {
    let handoffs = handoffs.display();
    let lessons = lessons.display();
    format!(
        "You are the weekly skill prune. Read north:harness/skills/agent-policy/SKILL.md and the verification and repo-safety skills first.
1. Measure: run `north skill-prune --measure`. It lists every shared SKILL.md body, marks domain skills (metadata.kind: domain, exempt from the 60-line playbook target) and totals the bodies beside the operating point.
2. Prune each playbook body toward 60 lines, a reported target that fails nothing (agent-config-check fails only over 500), and keep shrinking wherever a rule is duplicated, detailed or no longer true. Move detail into the skill's references/notes.md. Merge duplicated rules, keeping the copy in the owning skill. Delete a rule only after checking it against the current code and CLI help. Lose no live rule: under a dated heading in {handoffs}/skill-prune-moves.md, write each removed or moved line and where it went.
3. Fold {lessons}: a lesson that held in two or more independent runs (its `also` entries name other boxes from separate incidents) goes into the skill line it names; a lesson that did not hold is dropped; one with no second run 14 days after its date moves out marked `expired`, and a lead that sees it again moves it back with its `also` entry, which counts as its second run; a tool defect or a privacy, safety or data-loss slip still pending is filed as an issue for its owning lead. Move each folded, dropped or filed line, marked with where it went, to codex-lessons-folded.md beside it.
4. Edit each repository in its own lane: `git -C ~/code/REPO/main worktree add -b skill-prune-DATE ~/code/REPO/worktrees/skill-prune-DATE origin/main`. Commit there, land with `safe-push --to main`, then run `agents sync`.
5. End with one line per landed repository and its SHA.
Write policy prose as clear, complete, well-made sentences. Change nothing outside skills, their references, the hooks that restate a moved rule and the two lessons files."
    )
}

fn newline_count(path: &Path) -> usize {
    fs::read(path).map_or(0, |bytes| bytes.iter().filter(|byte| **byte == b'\n').count())
}

fn session(handoffs: &Path, lessons: &Path) -> String {
    let account = env_or("SKILL_PRUNE_ACCOUNT", || "a".into());
    let name = format!("skill-prune-{}", date("+%Y%m%d"));
    let launched = Command::new(north_program())
        .args(["fleet", "launch", "--provider", "claude", "--account"])
        .arg(account)
        .args(["--parent", "none", "--model", "claude-opus-5-5", "--tier-reason", "skill and policy prose goes to Opus, never a cheaper tier", "--name", &name])
        .arg(brief(handoffs, lessons))
        .stderr(Stdio::inherit())
        .output();
    let id = launched.ok().filter(|output| output.status.success()).map(|output| text(&output).trim_end_matches('\n').to_owned()).unwrap_or_default();
    if id.is_empty() {
        return "session failed to launch through north fleet".to_owned();
    }
    let program = north_program().to_string_lossy().into_owned();
    match process::run_within(&program, &["fleet", "show", &id, "--follow"], SESSION_LIMIT) {
        Some(output) if output.status.success() => text(&output).trim_end_matches('\n').to_owned(),
        Some(output) => format!("session failed: {}", child_failure("north fleet show", output.status.code(), &output.stderr)),
        None => "session failed: north fleet show did not finish".to_owned(),
    }
}

fn prune(paths: &Paths) -> Result<std::path::PathBuf, String> {
    let report = paths.handoffs.join("skill-prune.md");
    let lessons = paths.handoffs.join("codex-lessons.md");
    fs::create_dir_all(&paths.handoffs).map_err(|error| error.to_string())?;
    let before = measure(&paths.operator)?;
    let lessons_before = newline_count(&lessons);
    let out = session(&paths.handoffs, &lessons);
    let after = measure(&paths.operator)?;
    let lessons_after = newline_count(&lessons);
    let stamp = date("-Iminutes");
    let body = format!(
        "# Skill prune {stamp}\n\n## Bodies before\n\n```\n{before}\n```\n\n## Bodies after\n\n```\n{after}\n```\n\ncodex-lessons.md: {lessons_before} lines before, {lessons_after} after.\n\n## Session\n\n{out}\n"
    );
    fs::write(&report, body).map_err(|error| format!("{}: {error}", report.display()))?;
    Ok(report)
}
