//! The exhausted marks that keep the api pool off an organization Anthropic refused for low credit: one file per
//! organization id under `api-exhausted`, reading `SESSION DAY` and, when the organization declares its next credit,
//! ` until=DAY`, from which day the mark no longer holds. Hosts swap their marks on each fleet tick.
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

const UNTIL: &str = "until=";

/// Whether the mark for organization `id` under `dir` holds on `today`.
pub fn held(dir: &Path, id: &str, today: &str) -> bool {
    std::fs::read_to_string(dir.join(id)).is_ok_and(|mark| holds(&mark, today))
}

fn holds(mark: &str, today: &str) -> bool {
    mark.split_whitespace().find_map(|word| word.strip_prefix(UNTIL)).is_none_or(|until| today < until)
}

/// Marks organization `id` exhausted by `session` on `today` until `until`, unless a mark already holds; whether it wrote one.
pub fn mark(dir: &Path, id: &str, session: &str, today: &str, until: Option<&str>) -> io::Result<bool> {
    if held(dir, id, today) {
        return Ok(false);
    }
    let until = until.filter(|until| *until > today).map(|until| format!(" {UNTIL}{until}")).unwrap_or_default();
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(id), format!("{session} {today}{until}\n"))?;
    Ok(true)
}

/// Every mark under `dir` that holds on `today`, by organization id.
pub fn all(dir: &Path, today: &str) -> BTreeMap<String, String> {
    let files = std::fs::read_dir(dir).into_iter().flatten().flatten();
    files.filter_map(|file| Some((file.file_name().into_string().ok()?, std::fs::read_to_string(file.path()).ok()?))).filter(|(_, mark)| holds(mark, today)).collect()
}

/// Writes each of `marks` that holds on `today` and that `dir` holds none for; the ids it wrote.
pub fn adopt(dir: &Path, marks: &BTreeMap<String, String>, today: &str) -> io::Result<Vec<String>> {
    let mut adopted = Vec::new();
    for (id, mark) in marks.iter().filter(|(id, mark)| holds(mark, today) && !held(dir, id, today) && !id.contains(['/', '.'])) {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(id), mark)?;
        adopted.push(id.clone());
    }
    Ok(adopted)
}
