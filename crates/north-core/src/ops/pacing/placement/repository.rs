use super::field;
use std::path::Path;

pub fn repository(text: &str) -> String {
    let declared = field(text, "Repository");
    let source = if declared.is_empty() { issue_repository(field(text, "Item")) } else { declared.split_whitespace().next().unwrap_or_default() };
    let last = source.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    last.trim_end_matches(".git").trim().to_lowercase()
}

/// The `REPO` of an `[OWNER/]REPO#N` reference at the start of `text`; prose has none.
fn issue_repository(text: &str) -> &str {
    let reference = text.split_whitespace().next().unwrap_or_default();
    reference.split_once('#').filter(|(_, number)| number.starts_with(|c: char| c.is_ascii_digit())).map_or("", |(repo, _)| repo)
}

/// The brief with a `Repository:` line when it names none itself: the container under `code/` that
/// holds `cwd`, else the repository of `task`.
pub fn anchored(text: &str, cwd: Option<&Path>, task: Option<&str>) -> String {
    if !repository(text).is_empty() {
        return text.to_owned();
    }
    let container = cwd.and_then(|cwd| {
        let mut parts = cwd.components().map(|part| part.as_os_str().to_string_lossy());
        parts.by_ref().find(|part| part == "code")?;
        parts.next().map(|part| part.into_owned())
    });
    match container.as_deref().or_else(|| task.map(issue_repository)).filter(|repo| !repo.is_empty()) {
        Some(repo) => format!("{text}\nRepository: {repo}\n"),
        None => text.to_owned(),
    }
}
