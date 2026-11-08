//! The context pack a pack-arm brief carries: `north work brief pack` over the paths the brief's own
//! `Files:` line names, run in the repository's main checkout, appended after the brief.
use super::shares::PACK;
use super::tier::{self, Evidence, History};
use regex::Regex;
use std::process::Command;
use std::sync::LazyLock;

static PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:[\w.-]+/)+[\w.-]*\w\.[A-Za-z][A-Za-z0-9]{0,4}$").expect("path pattern"));
const MOST_PATHS: usize = 6;

/// The paths the brief's `Files:` line names, in order, without repeats.
pub fn files(brief: &str) -> Vec<String> {
    let Some(line) = brief.lines().find_map(|line| line.strip_prefix("Files:")) else { return Vec::new() };
    let mut paths: Vec<String> = Vec::new();
    for word in line.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '`' | '(' | ')')) {
        let word = word.trim_end_matches(['.', ':']);
        if PATH.is_match(word) && !paths.iter().any(|known| known == word) {
            paths.push(word.to_owned());
        }
    }
    paths.truncate(MOST_PATHS);
    paths
}

/// `brief` stamped with the context arm drawn for it, and, when its `Files:` line names paths to pack,
/// with the pack arm drawn for `item` ahead of it (the ledger reads a run's first `Arm:` line, and the
/// context window reads any), plus the pack on the pack arm; the words naming each draw come second.
pub fn armed(brief: &str, context: (&str, &str), item: &str, category: &str, history: &History, evidence: &Evidence) -> (String, String) {
    let (context, drawn) = context;
    let stamped = super::brief::armed(brief, context);
    let mut why = format!("context arm {context} {drawn}");
    if files(brief).is_empty() {
        return (stamped, why);
    }
    let (arm, drawn) = tier::pack_arm(item, category, history, evidence);
    why.push_str(&format!(", pack arm {arm} {drawn}"));
    (appended(&super::brief::armed(&stamped, &arm), &arm, item), why)
}

/// `brief` followed by the pack for its `Files:` paths when `arm` is the pack arm and the pack prints
/// anything; the brief itself otherwise.
pub fn appended(brief: &str, arm: &str, item: &str) -> String {
    if arm != PACK {
        return brief.to_owned();
    }
    let paths = files(brief);
    if paths.is_empty() {
        return brief.to_owned();
    }
    let Some((repo, number)) = item.rsplit_once('#') else { return brief.to_owned() };
    let repo = repo.rsplit('/').next().unwrap_or(repo);
    let Some(home) = std::env::var_os("HOME") else { return brief.to_owned() };
    let checkout = std::path::Path::new(&home).join("code").join(crate::threads::catalog::checkout(repo)).join("main");
    let mut command = Command::new(std::env::current_exe().unwrap_or_else(|_| "north".into()));
    command.args(["work", "brief", "pack"]).args(["--issue", &format!("{repo}#{number}")]).args(&paths).current_dir(checkout);
    match command.output() {
        Ok(output) if output.status.success() && !output.stdout.is_empty() => format!("{brief}\n\nContext pack for the Files: paths (`north work brief pack`, read from the repository, its coverage map and the fact graph; a graph line marked as a claim is quoted, not observed):\n{}", String::from_utf8_lossy(&output.stdout).trim_end()),
        _ => brief.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pack_reads_the_paths_of_the_files_line_and_only_the_pack_arm_carries_it() {
        let brief = "Item: north#882 x\nFiles: the pack crate, crates/north-graph-ground/src/reach.rs and file.rs (read-only reuse), `crates/north-dispatch/src/dispatch/brief.rs`, docs/rationale/experiments.toml.\nTier: sonnet-high";
        assert_eq!(files(brief), ["crates/north-graph-ground/src/reach.rs", "crates/north-dispatch/src/dispatch/brief.rs", "docs/rationale/experiments.toml"]);
        assert!(files("Files: as north#882 names them").is_empty());
        assert_eq!(appended(brief, super::super::shares::NO_PACK, "north#882"), brief);
    }
}
