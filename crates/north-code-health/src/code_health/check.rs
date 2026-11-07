use super::config::{Kind, in_worktree};
use super::functions::{SourceText, measure};
use super::gate::{UNMAPPED, Violation, cycle_text, file_size, function_debt, layer_text};
use super::graph::layer_violation;
use super::resolver::Resolver;
use super::snapshot::{Snapshot, line_count};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const CYCLE_SEARCH_LIMIT: usize = 3000;

fn relative(repo: &Path, file: &Path) -> String {
    let absolute = if file.is_absolute() { file.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(file) };
    let normal: PathBuf = absolute.components().fold(PathBuf::new(), |mut out, part| {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
        out
    });
    normal.strip_prefix(repo).map_or_else(|_| normal.to_string_lossy().into_owned(), |rest| rest.to_string_lossy().into_owned())
}

pub(super) fn check(repo: &Path, file: &Path, override_path: Option<&Path>) -> Result<Vec<Violation>, String> {
    let rel = relative(repo, file);
    let config = in_worktree(repo, override_path)?;
    if !config.present || !repo.join(&rel).exists() {
        return Ok(Vec::new());
    }
    let exists = |path: &str| repo.join(path).exists();
    let is_hand = |path: &str| config.classify(path, exists) == Kind::Hand;
    if !is_hand(&rel) {
        return Ok(Vec::new());
    }
    let head = Snapshot::new(repo, "HEAD")?;
    let work: std::rc::Rc<str> = std::fs::read_to_string(repo.join(&rel)).map_err(|error| format!("{rel}: {error}"))?.into();
    let before = head.text(&rel);
    let t = config.thresholds_of(&rel);
    let mut v: Vec<Violation> = Vec::new();
    v.extend(file_size(&rel, line_count(&work), before.as_deref().map(line_count), t.file_lines));
    let head_files = [SourceText { path: rel.clone(), text: work.clone() }];
    let base_files: Vec<SourceText> = before.iter().map(|text| SourceText { path: rel.clone(), text: text.clone() }).collect();
    let measured = measure(&[("h", &head_files), ("b", &base_files)])?;
    v.extend(function_debt(&rel, measured.get(&format!("h/{rel}")).map_or(&[][..], Vec::as_slice), measured.get(&format!("b/{rel}")).map_or(&[][..], Vec::as_slice), &t));
    let resolver = Resolver::of_worktree(repo, &head);
    let read = |path: &str| std::fs::read_to_string(repo.join(path)).unwrap_or_default();
    let old: HashSet<String> = before.as_deref().map(|text| resolver.imports(&rel, text).into_iter().collect()).unwrap_or_default();
    for to in resolver.imports(&rel, &work) {
        if old.contains(&to) || !is_hand(&to) {
            continue;
        }
        if let Some(lv) = layer_violation(&config, &rel, &to) {
            v.push(Violation::new("layer", &rel, layer_text(&to, &lv.from_module, &lv.to_module)));
        }
        let mut seen: HashSet<String> = HashSet::from([to.clone()]);
        let mut frontier = vec![to.clone()];
        while !frontier.is_empty() && seen.len() < CYCLE_SEARCH_LIMIT {
            if frontier.contains(&rel) {
                v.push(Violation::new("cycle", &rel, cycle_text(&to)));
                break;
            }
            let mut next = Vec::new();
            for node in &frontier {
                for found in resolver.imports(node, &read(node)) {
                    if !seen.contains(&found) && is_hand(&found) {
                        seen.insert(found.clone());
                        next.push(found);
                    }
                }
            }
            frontier = next;
        }
    }
    if before.is_none() && !config.modules.is_empty() && config.module_of(&rel).is_none() {
        v.push(Violation::new("layer", &rel, UNMAPPED.to_owned()));
    }
    Ok(v)
}
