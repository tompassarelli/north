//! The exhausted marks that keep the api pool off an organization Anthropic refused for low credit: one file per
//! organization id under `api-exhausted`, whose first line reads `SESSION DAY` and, when the organization declares its
//! next credit, ` until=DAY` from which day the mark no longer holds, followed by the provider's refusal body verbatim.
//! Hosts swap their marks on each fleet tick; a passing probe clears the mark and leaves a tombstone of the text it
//! cleared, so a peer that still holds that same mark cannot restore it.
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

const UNTIL: &str = "until=";
const CLEARED: &str = "cleared";
const LOW_CREDIT: &str = "credit balance is too low";

/// Whether a refusal body names the API's low-credit error, the only refusal that writes a mark.
pub fn names_low_credit(body: &str) -> bool {
    body.to_lowercase().contains(LOW_CREDIT)
}

fn meta(mark: &str) -> &str {
    mark.lines().next().unwrap_or("")
}

/// The provider's refusal body a mark keeps below its first line.
pub fn body(mark: &str) -> &str {
    mark.split_once('\n').map_or("", |(_, body)| body)
}

/// Whether the mark for organization `id` under `dir` holds on `today`.
pub fn held(dir: &Path, id: &str, today: &str) -> bool {
    std::fs::read_to_string(dir.join(id)).is_ok_and(|mark| holds(&mark, today))
}

fn holds(mark: &str, today: &str) -> bool {
    meta(mark).split_whitespace().find_map(|word| word.strip_prefix(UNTIL)).is_none_or(|until| today < until)
}

/// Marks organization `id` exhausted by `session` on `today` until `until`, keeping `body`; a body that does not name low credit writes nothing, and a mark that already holds is left alone. Whether it wrote.
pub fn mark(dir: &Path, id: &str, session: &str, today: &str, until: Option<&str>, body: &str) -> io::Result<bool> {
    if !names_low_credit(body) || held(dir, id, today) {
        return Ok(false);
    }
    let until = until.filter(|until| *until > today).map(|until| format!(" {UNTIL}{until}")).unwrap_or_default();
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(id), format!("{session} {today}{until}\n{}\n", body.trim_end()))?;
    Ok(true)
}

/// The day from which the mark for organization `id` under `dir` stops holding, when it names one.
pub fn until(dir: &Path, id: &str) -> Option<String> {
    let mark = std::fs::read_to_string(dir.join(id)).ok()?;
    meta(&mark).split_whitespace().find_map(|word| word.strip_prefix(UNTIL)).map(str::to_owned)
}

/// Every mark under `dir` that holds on `today`, by organization id.
pub fn all(dir: &Path, today: &str) -> BTreeMap<String, String> {
    let files = std::fs::read_dir(dir).into_iter().flatten().flatten();
    files.filter(|file| file.path().is_file()).filter_map(|file| Some((file.file_name().into_string().ok()?, std::fs::read_to_string(file.path()).ok()?))).filter(|(_, mark)| holds(mark, today)).collect()
}

/// Clears the mark for organization `id` under `dir` because a probe passed at `at` (Unix seconds), leaving the text it cleared as a tombstone; whether a mark was cleared.
pub fn clear(dir: &Path, id: &str, at: i64) -> io::Result<bool> {
    let Ok(mark) = std::fs::read_to_string(dir.join(id)) else { return Ok(false) };
    let cleared = dir.join(CLEARED);
    std::fs::create_dir_all(&cleared)?;
    let stamp = crate::foundation::clock::Timestamp::from_unix_seconds(at).iso();
    std::fs::write(cleared.join(id), format!("{stamp}\n{}\n", mark.trim_end()))?;
    std::fs::remove_file(dir.join(id))?;
    Ok(true)
}

/// The exact mark text a clear left behind for organization `id`, if any.
pub fn cleared(dir: &Path, id: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(CLEARED).join(id)).ok()?;
    text.split_once('\n').map(|(_, mark)| mark.trim_end().to_owned())
}

/// Writes each of `marks` that holds on `today` and that `dir` holds none for, except one a probe already cleared with exactly this text, so a peer's stale copy does not bring a cleared mark back; the ids it wrote.
pub fn adopt(dir: &Path, marks: &BTreeMap<String, String>, today: &str) -> io::Result<Vec<String>> {
    let mut adopted = Vec::new();
    for (id, mark) in marks.iter().filter(|(id, mark)| holds(mark, today) && !held(dir, id, today) && !id.contains(['/', '.'])) {
        if cleared(dir, id).is_some_and(|cleared| cleared == mark.trim_end()) {
            continue;
        }
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(id), format!("{}\n", mark.trim_end()))?;
        adopted.push(id.clone());
    }
    Ok(adopted)
}

#[cfg(test)]
mod tests;
