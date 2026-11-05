use std::fs;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{Date, Month, OffsetDateTime};

pub(crate) const GUIDES_DIR: &str = "harness/skills/agent-policy/references/guides";
const STALE_DAYS: i64 = 90;
const MANIFEST: &str = "sources.json";

struct Source {
    repo: &'static str,
    path: &'static str,
    out: &'static str,
    license: &'static str,
}

const SOURCES: [Source; 6] = [
    Source { repo: "agentskills/agentskills", path: "docs/specification.mdx", out: "agentskills-specification.md", license: "Apache-2.0" },
    Source { repo: "agentskills/agentskills", path: "docs/skill-creation/best-practices.mdx", out: "agentskills-best-practices.md", license: "Apache-2.0" },
    Source { repo: "agentskills/agentskills", path: "docs/skill-creation/optimizing-descriptions.mdx", out: "agentskills-optimizing-descriptions.md", license: "Apache-2.0" },
    Source { repo: "agentskills/agentskills", path: "docs/skill-creation/evaluating-skills.mdx", out: "agentskills-evaluating-skills.md", license: "Apache-2.0" },
    Source { repo: "anthropics/skills", path: "skills/skill-creator/SKILL.md", out: "anthropic-skill-creator.md", license: "Apache-2.0" },
    Source { repo: "openai/openai-cookbook", path: "examples/gpt-5/codex_prompting_guide.ipynb", out: "openai-codex-prompting-guide.md", license: "MIT" },
];

#[derive(Serialize, Deserialize)]
pub(crate) struct Entry {
    pub(crate) out: String,
    pub(crate) url: String,
    pub(crate) license: String,
    pub(crate) retrieved: String,
}

pub(crate) struct Staleness {
    pub(crate) lines: Vec<String>,
    pub(crate) stale: usize,
}

pub(crate) fn staleness(manifest: &[Entry], today: Date) -> Result<Staleness, String> {
    let mut lines = Vec::new();
    let mut stale = 0;
    for entry in manifest {
        let age = (today - date(&entry.retrieved)?).whole_days();
        stale += usize::from(age > STALE_DAYS);
        lines.push(format!("{age:>4} days  {}", entry.out));
    }
    if stale > 0 {
        lines.push(format!(
            "{stale} guide(s) older than {STALE_DAYS} days: run `north config check guides --refresh`, then re-check the harness against them."
        ));
    }
    Ok(Staleness { lines, stale })
}

pub(crate) fn check(guides: &Path) -> Result<bool, String> {
    let path = guides.join(MANIFEST);
    let manifest: Vec<Entry> = serde_json::from_str(&fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let report = staleness(&manifest, OffsetDateTime::now_utc().date())?;
    for line in &report.lines {
        println!("{line}");
    }
    Ok(report.stale == 0)
}

pub(crate) fn refresh(guides: &Path) -> Result<(), String> {
    let retrieved = iso(OffsetDateTime::now_utc().date());
    fs::create_dir_all(guides).map_err(|error| format!("{}: {error}", guides.display()))?;
    let mut manifest = Vec::new();
    for source in &SOURCES {
        let commit: Value = serde_json::from_str(&fetch(&format!("https://api.github.com/repos/{}/commits/HEAD", source.repo))?)
            .map_err(|error| format!("{}: {error}", source.repo))?;
        let sha = commit["sha"].as_str().ok_or_else(|| format!("{}: HEAD commit has no sha", source.repo))?;
        let raw = fetch(&format!("https://raw.githubusercontent.com/{}/{sha}/{}", source.repo, source.path))?;
        let body = if source.path.ends_with(".ipynb") { notebook_markdown(&raw)? } else { raw };
        let url = format!("https://github.com/{}/blob/{sha}/{}", source.repo, source.path);
        let header = format!(
            "<!-- Vendored from {url} ({}), retrieved {retrieved}. Refresh with `north config check guides --refresh`; do not edit. -->\n\n",
            source.license
        );
        let out = guides.join(source.out);
        fs::write(&out, format!("{header}{}\n", body.trim_end())).map_err(|error| format!("{}: {error}", out.display()))?;
        println!("{} ← {}@{}", source.out, source.repo, &sha[..sha.len().min(12)]);
        manifest.push(Entry { out: source.out.to_owned(), url, license: source.license.to_owned(), retrieved: retrieved.clone() });
    }
    let path = guides.join(MANIFEST);
    let json = serde_json::to_string_pretty(&manifest).map_err(|error| error.to_string())?;
    fs::write(&path, json + "\n").map_err(|error| format!("{}: {error}", path.display()))
}

pub(crate) fn notebook_markdown(text: &str) -> Result<String, String> {
    let notebook: Value = serde_json::from_str(text).map_err(|error| format!("notebook: {error}"))?;
    let cells = notebook["cells"].as_array().ok_or("notebook has no cells")?;
    let blocks: Vec<String> = cells
        .iter()
        .map(|cell| {
            let body = match &cell["source"] {
                Value::Array(lines) => lines.iter().filter_map(Value::as_str).collect::<String>(),
                other => other.as_str().unwrap_or("").to_owned(),
            };
            if cell["cell_type"] == "code" { format!("```\n{body}\n```") } else { body }
        })
        .collect();
    Ok(blocks.join("\n\n"))
}

fn fetch(url: &str) -> Result<String, String> {
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", "60", "-A", "agent-machinery-refresh-guides", url])
        .output()
        .map_err(|error| format!("{url}: curl: {error}"))?;
    if !output.status.success() {
        return Err(format!("{url}: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    String::from_utf8(output.stdout).map_err(|error| format!("{url}: {error}"))
}

fn date(iso: &str) -> Result<Date, String> {
    let invalid = || format!("invalid retrieved date {iso}");
    let mut parts = iso.splitn(3, '-').map(str::parse::<u32>);
    let (Some(Ok(year)), Some(Ok(month)), Some(Ok(day))) = (parts.next(), parts.next(), parts.next()) else {
        return Err(invalid());
    };
    let month = Month::try_from(u8::try_from(month).map_err(|_| invalid())?).map_err(|_| invalid())?;
    Date::from_calendar_date(i32::try_from(year).map_err(|_| invalid())?, month, u8::try_from(day).map_err(|_| invalid())?)
        .map_err(|_| invalid())
}

fn iso(date: Date) -> String {
    format!("{:04}-{:02}-{:02}", date.year(), u8::from(date.month()), date.day())
}
