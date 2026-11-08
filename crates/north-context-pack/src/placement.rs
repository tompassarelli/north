use std::path::Path;

const MOST_NAMES: usize = 10;

pub fn line(root: &Path, path: &str) -> Option<String> {
    let structure = std::fs::read_to_string(root.join("structure.toml")).ok()?;
    let placed = north_code_health::code_health::placement(&structure, path)?;
    let shown: Vec<&str> = placed.may_use.iter().take(MOST_NAMES).map(String::as_str).collect();
    let more = placed.may_use.len().saturating_sub(MOST_NAMES);
    let tail = if more > 0 { format!(" and {more} more") } else { String::new() };
    let uses = if shown.is_empty() { "nothing".to_owned() } else { shown.join(", ") };
    Some(format!("module {}, may use {uses}{tail}", placed.module))
}
