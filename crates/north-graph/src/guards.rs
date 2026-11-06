use std::path::Path;

use crate::query::parse_query;
use crate::store::Graph;
use crate::term::Term;

pub struct Finding {
    pub guard: String,
    pub row: Vec<Term>,
}

pub struct Verdict {
    pub guards: usize,
    pub findings: Vec<Finding>,
}

pub fn check(graph: &Graph, directory: &Path) -> Result<Verdict, String> {
    let mut paths: Vec<_> = std::fs::read_dir(directory)
        .map_err(|why| format!("{}: {why}", directory.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .collect();
    paths.sort();
    let mut verdict = Verdict { guards: paths.len(), findings: Vec::new() };
    for path in paths {
        let guard = path.file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let text = std::fs::read_to_string(&path).map_err(|why| format!("{}: {why}", path.display()))?;
        let query = parse_query(&text).map_err(|why| format!("guard {guard}: {why}"))?;
        let answer = graph.ask(&query).map_err(|why| format!("guard {guard}: {why}"))?;
        verdict.findings.extend(answer.rows.into_iter().map(|row| Finding { guard: guard.clone(), row }));
    }
    Ok(verdict)
}
