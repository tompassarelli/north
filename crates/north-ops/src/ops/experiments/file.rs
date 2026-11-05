//! Files each due verdict once as a box on the entry's owning issue: keep as a ticked box, revert as
//! a revert box with the numbers, decide as one line for Tom with a recommendation, and re-measure.
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use super::registry::{Experiment, Issue, Metric};
use super::verdict::{Judged, Reading, Recommend, Verdict};

pub fn marker(entry: &Experiment) -> String {
    format!("<!-- experiment {}@{} -->", entry.id, entry.review.text())
}

fn reading(entry: &Experiment, reading: Option<Reading>, window: &str) -> String {
    match reading {
        Some(Reading { value: Some(value), n }) => format!("{value:.3} (n={n}, {window})"),
        Some(Reading { value: None, n }) => format!("no value (n={n}, {window})"),
        None => format!("unread ({window}, {})", entry.metric.name()),
    }
}

/// The numbers a box carries: the metric before and after and the move toward the prediction.
pub fn numbers(entry: &Experiment, before: Option<Reading>, after: Option<Reading>, judged: &Judged) -> String {
    if let Metric::Unread(measure) = &entry.metric {
        return format!("no ledger reading; judged by {measure} over {}", entry.after.text());
    }
    let base = entry.before.map_or_else(|| "baseline 0".to_owned(), |window| reading(entry, before, &window.text()));
    let moved = judged.moved.map_or_else(String::new, |moved| format!(", moved {moved:.3} toward the predicted {} against a bar of {}", entry.direction.word(), entry.bar));
    let over = match &entry.metric {
        Metric::IncidentRows(class) => format!("incident class {class}"),
        _ => entry.scope.text(),
    };
    format!("{} over {over}: {base} -> {}{moved}", entry.metric.name(), reading(entry, after, &entry.after.text()))
}

pub fn line(entry: &Experiment, before: Option<Reading>, after: Option<Reading>, judged: &Judged) -> String {
    let numbers = numbers(entry, before, after, judged);
    let id = &entry.id;
    let tail = format!("origin experiment {id} {}", marker(entry));
    match &judged.verdict {
        Verdict::Keep => format!("- [x] Keep {id} ({}): {numbers}; kept on its review {}, {tail}", entry.landed, entry.review.text()),
        Verdict::Revert => format!("- [ ] Revert {} ({}): {numbers}; it moved against the prediction, {tail}", entry.landed, entry.change),
        Verdict::Decide(recommend) => {
            let advice = match recommend {
                Recommend::Keep => "keep, since it moved the predicted way but short of the bar",
                Recommend::Revert => "revert, since it did not move the predicted way, and a rule that does not pay off is machinery to carry",
            };
            format!("- [ ] Tom decides {id} ({}): {numbers}; recommend {advice}, {tail}", entry.change)
        }
        Verdict::ReMeasure(why) => format!("- [ ] Re-measure {id}: {why}; record the reading, then set a new review date in docs/rationale/experiments.toml, {tail}"),
    }
}

/// The owning issues, as far as filing needs them.
pub trait Issues {
    fn body(&self, issue: &Issue) -> Result<(String, bool), String>;
    fn set_body(&self, issue: &Issue, body: &str) -> Result<(), String>;
    fn reopen(&self, issue: &Issue) -> Result<(), String>;
}

/// The verdict keys already filed on this host, so a filed verdict is never read or filed again.
pub struct Filed {
    path: PathBuf,
    keys: BTreeSet<String>,
}

impl Filed {
    pub fn load(path: PathBuf) -> Self {
        let keys = fs::read_to_string(&path).map(|text| text.lines().map(str::to_owned).collect()).unwrap_or_default();
        Self { path, keys }
    }

    pub fn key(entry: &Experiment) -> String {
        format!("{}@{}", entry.id, entry.review.text())
    }

    pub fn has(&self, entry: &Experiment) -> bool {
        self.keys.contains(&Self::key(entry))
    }

    fn record(&mut self, entry: &Experiment) -> Result<(), String> {
        self.keys.insert(Self::key(entry));
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|why| format!("{}: {why}", parent.display()))?;
        }
        let text: String = self.keys.iter().map(|key| format!("{key}\n")).collect();
        let staged = self.path.with_extension("new");
        fs::write(&staged, text).and_then(|()| fs::rename(&staged, &self.path)).map_err(|why| format!("{}: {why}", self.path.display()))
    }
}

/// Appends the box unless the issue already carries this verdict's marker, reopens a closed issue
/// for a box left open, and records the key either way.
pub fn file(issues: &dyn Issues, filed: &mut Filed, entry: &Experiment, line: &str, verdict: &Verdict) -> Result<String, String> {
    let (body, open) = issues.body(&entry.owner)?;
    let issue = entry.owner.text();
    if body.contains(&marker(entry)) {
        filed.record(entry)?;
        return Ok(format!("{}: already on {issue}", entry.id));
    }
    issues.set_body(&entry.owner, &format!("{}\n{line}\n", body.trim_end()))?;
    if !open && *verdict != Verdict::Keep {
        issues.reopen(&entry.owner)?;
    }
    filed.record(entry)?;
    Ok(format!("{}: filed {} on {issue}", entry.id, verdict.word()))
}

/// Issues through `gh`.
pub struct GitHub;

fn gh(args: &[&str]) -> Result<String, String> {
    let output = crate::platform::github::run(args).map_err(|why| format!("cannot run gh: {why}"))?;
    if !output.status.success() {
        return Err(format!("gh {}: {}", args.iter().take(2).copied().collect::<Vec<_>>().join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

impl Issues for GitHub {
    fn body(&self, issue: &Issue) -> Result<(String, bool), String> {
        let number = issue.number.to_string();
        let text = gh(&["issue", "view", &number, "-R", &issue.repo, "--json", "body,state"])?;
        let value: serde_json::Value = serde_json::from_str(&text).map_err(|why| format!("gh issue view {}: {why}", issue.text()))?;
        Ok((value["body"].as_str().unwrap_or_default().to_owned(), value["state"].as_str() == Some("OPEN")))
    }

    fn set_body(&self, issue: &Issue, body: &str) -> Result<(), String> {
        gh(&["issue", "edit", &issue.number.to_string(), "-R", &issue.repo, "--body", body]).map(drop)
    }

    fn reopen(&self, issue: &Issue) -> Result<(), String> {
        gh(&["issue", "reopen", &issue.number.to_string(), "-R", &issue.repo]).map(drop)
    }
}
