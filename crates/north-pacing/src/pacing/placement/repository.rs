use super::field;
use std::path::Path;

pub fn repository(text: &str) -> String {
    let declared = field(text, "Repository");
    let source = if declared.is_empty() { issue_repository(field(text, "Item")) } else { declared.split_whitespace().next().unwrap_or_default() };
    canonical(source)
}

fn canonical(source: &str) -> String {
    let last = source.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    let name = last.trim_end_matches(".git").trim().to_lowercase();
    crate::threads::catalog::CATALOG.canonical(&name).to_owned()
}

/// The `REPO` of an `[OWNER/]REPO#N` reference at the start of `text`; prose has none.
fn issue_repository(text: &str) -> &str {
    let reference = text.split_whitespace().next().unwrap_or_default();
    reference.split_once('#').filter(|(_, number)| number.starts_with(|c: char| c.is_ascii_digit())).map_or("", |(repo, _)| repo)
}

/// The repository a launch itself carries, never one its text names: the repository of `task`, else the container under `code/` that holds `cwd`, in its catalog spelling; empty when neither names one.
pub fn launch_repository(cwd: Option<&Path>, task: Option<&str>) -> String {
    let container = cwd.and_then(|cwd| {
        let mut parts = cwd.components().map(|part| part.as_os_str().to_string_lossy());
        parts.by_ref().find(|part| part == "code")?;
        parts.next().map(|part| part.into_owned())
    });
    let source = task.map(issue_repository).filter(|repo| !repo.is_empty()).or(container.as_deref()).unwrap_or_default();
    canonical(source)
}

/// The brief with a `Repository:` line when it names none itself, from the launch's own facts.
pub fn anchored(text: &str, cwd: Option<&Path>, task: Option<&str>) -> String {
    if !repository(text).is_empty() {
        return text.to_owned();
    }
    match launch_repository(cwd, task) {
        repo if repo.is_empty() => text.to_owned(),
        repo => format!("{text}\nRepository: {repo}\n"),
    }
}
