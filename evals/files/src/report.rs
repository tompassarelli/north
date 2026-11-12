use std::collections::BTreeSet;
use std::fmt::Write;

use crate::cap;
use crate::variants;
use crate::task::Tree;
use crate::variants::Ranked;

pub struct TaskResult<'a> {
    pub id: String,
    pub truth: BTreeSet<String>,
    pub tree: &'a Tree,
    pub sources: Vec<Ranked>,
    pub unions: Vec<Ranked>,
    pub map_bytes: usize,
}

impl TaskResult<'_> {
    fn all(&self) -> impl Iterator<Item = &Ranked> {
        self.sources.iter().chain(&self.unions)
    }

    fn kept(&self, ranked: &Ranked) -> Vec<String> {
        cap::apply(&ranked.files, self.tree).0
    }

    fn found(&self, ranked: &Ranked) -> usize {
        self.kept(ranked).iter().filter(|path| self.truth.contains(*path)).count()
    }

    fn miss_reason(&self, ranked: &Ranked, path: &str) -> &'static str {
        if path.starts_with("wisp:") {
            return "edited in the other repository";
        }
        let Some(&size) = self.tree.sizes.get(path) else { return "new at the base commit" };
        if size > cap::FILE_BYTES {
            return "over the 8 KB file cap";
        }
        let (_, dropped) = cap::apply(&ranked.files, self.tree);
        if dropped.iter().any(|(dropped, _)| dropped == path) {
            "dropped by the 32 KB total"
        } else if !ranked.visible.contains(path) {
            "not in what the picker saw"
        } else {
            "seen but not proposed"
        }
    }
}

fn percent(part: usize, whole: usize) -> String {
    format!("{part} of {whole} ({:.0}%)", 100.0 * part as f64 / whole.max(1) as f64)
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let all: Vec<f64> = values.collect();
    all.iter().sum::<f64>() / all.len().max(1) as f64
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

fn names(results: &[TaskResult]) -> Vec<String> {
    results[0].all().map(|ranked| ranked.name.clone()).collect()
}

fn by_name<'a>(result: &'a TaskResult, name: &str) -> &'a Ranked {
    result.all().find(|ranked| ranked.name == name).expect("variant present")
}

fn table(results: &[TaskResult], total: usize) -> String {
    let mut out = String::from("| picker | edited files found | tasks with a hit | files kept per task | files named per task | ceiling |\n|---|---|---|---|---|---|\n");
    for name in names(results) {
        let found: usize = results.iter().map(|r| r.found(by_name(r, &name))).sum();
        let hit = results.iter().filter(|r| r.found(by_name(r, &name)) > 0).count();
        let kept = mean(results.iter().map(|r| r.kept(by_name(r, &name)).len() as f64));
        let named = mean(results.iter().map(|r| by_name(r, &name).files.len() as f64));
        let ceiling: usize = results.iter().map(|r| cap::reachable(&by_name(r, &name).visible, &r.truth, r.tree)).sum();
        let _ = writeln!(out, "| {name} | {} | {hit} of {} | {kept:.1} | {named:.1} | {} |", percent(found, total), results.len(), percent(ceiling, total));
    }
    out
}

fn cost(results: &[TaskResult], name: &str) -> String {
    let replies: Vec<&serde_json::Value> = results.iter().filter_map(|r| by_name(r, name).classifier.as_ref()).collect();
    let number = |key: &'static str| replies.iter().map(move |reply| reply[key].as_f64().unwrap_or(0.0)).collect::<Vec<f64>>();
    let invented: usize = results.iter().map(|r| by_name(r, name).invented).sum();
    format!(
        "- {name}: wall {:.1} s median, {:.1} s mean; {:.0} input and {:.0} output tokens per task; {invented} invented paths over {} tasks",
        median(number("wall_ms")) / 1000.0,
        mean(number("wall_ms").into_iter()) / 1000.0,
        mean(number("input_tokens").into_iter()),
        mean(number("output_tokens").into_iter()),
        results.len()
    )
}

fn per_task(results: &[TaskResult]) -> String {
    let mut out = format!("task | edited | {}\n", names(results).join(" | "));
    for r in results {
        let cells: Vec<String> = r.all().map(|ranked| r.found(ranked).to_string()).collect();
        let _ = writeln!(out, "{} | {} | {}", r.id, r.truth.len(), cells.join(" | "));
    }
    out
}

fn misses(results: &[TaskResult], name: &str) -> String {
    let mut out = format!("Misses of {name}, per task (edited file: why, then what it kept):\n");
    for r in results {
        let ranked = by_name(r, name);
        let kept: BTreeSet<String> = r.kept(ranked).into_iter().collect();
        let missed: Vec<String> = r.truth.iter().filter(|path| !kept.contains(*path)).map(|path| format!("{path} [{}]", r.miss_reason(ranked, path))).collect();
        if !missed.is_empty() {
            let _ = writeln!(out, "- {}: {} | kept: {}", r.id, missed.join("; "), kept.iter().cloned().collect::<Vec<_>>().join(", "));
        }
    }
    out
}

fn best_union(results: &[TaskResult]) -> String {
    results[0].unions.iter().map(|u| u.name.clone()).max_by_key(|name| results.iter().map(|r| r.found(by_name(r, name))).sum::<usize>()).unwrap_or_default()
}

const NAMED_FOR_UNION: usize = 10;
const MAP_AND_SEARCH: &str = "map + top 10 of text search";
const SEARCH_TOP: &str = "text search, top 10";

fn directory(path: &str) -> String {
    path.rsplit_once('/').map_or_else(String::new, |(dir, _)| dir.to_owned())
}

fn unique_directories(paths: impl Iterator<Item = String>) -> BTreeSet<String> {
    paths.map(|path| directory(&path)).collect()
}

struct Overlap {
    named: usize,
    changed: usize,
    hit: usize,
}

impl Overlap {
    fn of(named: &BTreeSet<String>, changed: &BTreeSet<String>) -> Overlap {
        Overlap { named: named.len(), changed: changed.len(), hit: named.intersection(changed).count() }
    }

    fn recall(&self) -> f64 {
        self.hit as f64 / self.changed.max(1) as f64
    }

    fn precision(&self) -> f64 {
        if self.named == 0 { 0.0 } else { self.hit as f64 / self.named as f64 }
    }
}

fn named_by(result: &TaskResult, name: &str) -> BTreeSet<String> {
    let search = &by_name(result, variants::GREP).files;
    let top: Vec<String> = search.iter().take(NAMED_FOR_UNION).cloned().collect();
    match name {
        MAP_AND_SEARCH => cap::union_in_order(&[&by_name(result, variants::MAP).files, &top]).into_iter().collect(),
        SEARCH_TOP => top.into_iter().collect(),
        _ => by_name(result, name).files.iter().cloned().collect(),
    }
}

fn uncapped_row(results: &[TaskResult], name: &str, by_directory: bool) -> String {
    let overlaps: Vec<Overlap> = results.iter().map(|r| {
        let (named, changed) = (named_by(r, name), r.truth.clone());
        if by_directory {
            Overlap::of(&unique_directories(named.into_iter()), &unique_directories(changed.into_iter()))
        } else {
            Overlap::of(&named, &changed)
        }
    }).collect();
    let (hit, changed, named): (usize, usize, usize) = overlaps.iter().fold((0, 0, 0), |sum, o| (sum.0 + o.hit, sum.1 + o.changed, sum.2 + o.named));
    format!(
        "| {name} | {} | {:.0}% | {:.0}% | {:.0}% | {:.0}% | {:.1} |",
        if by_directory { "directory" } else { "file" },
        100.0 * hit as f64 / changed.max(1) as f64,
        100.0 * median(overlaps.iter().map(Overlap::recall).collect()),
        100.0 * hit as f64 / named.max(1) as f64,
        100.0 * median(overlaps.iter().map(Overlap::precision).collect()),
        named as f64 / results.len() as f64
    )
}

fn uncapped(results: &[TaskResult]) -> String {
    let mut out = String::from("Without the size cap, against all changed files (including new files and the other repository's), per named file or directory:\n\n| picker | counted by | recall overall | recall per-task median | precision overall | precision per-task median | named per task |\n|---|---|---|---|---|---|---|\n");
    for name in [variants::MAP, MAP_AND_SEARCH, SEARCH_TOP] {
        for by_directory in [false, true] {
            let _ = writeln!(out, "{}", uncapped_row(results, name, by_directory));
        }
    }
    out
}

pub fn summary(results: &[TaskResult], model: &str) -> String {
    let total: usize = results.iter().map(|r| r.truth.len()).sum();
    let reachable: usize = results.iter().map(|r| r.truth.iter().filter(|p| r.tree.sizes.get(*p).is_some_and(|&s| s <= cap::FILE_BYTES)).count()).sum();
    let grep: usize = results.iter().map(|r| r.found(by_name(r, crate::variants::GREP))).sum();
    let best = best_union(results);
    let best_found: usize = results.iter().map(|r| r.found(by_name(r, &best))).sum();
    let mut out = format!("{} tasks, {total} edited files; at most {} could be kept by any picker under the 8 KB file cap. Classifier model {model}; map {:.0} KB per task on average.\n\n", results.len(), percent(reachable, total), mean(results.iter().map(|r| r.map_bytes as f64)) / 1024.0);
    out.push_str(&table(results, total));
    let verdict = if best_found >= 2 * grep { "at least twice text search" } else { "less than twice text search" };
    let _ = write!(out, "\n{}\n{}\n\nBest union: {best}, {best_found} files against text search's {grep}; {verdict}.\n\n", cost(results, crate::variants::MAP), cost(results, crate::variants::CANDIDATES));
    out.push_str(&uncapped(results));
    out.push('\n');
    out.push_str(&per_task(results));
    for name in [crate::variants::MAP, crate::variants::CANDIDATES] {
        out.push('\n');
        out.push_str(&misses(results, name));
    }
    out.push('\n');
    out.push_str(&misses(results, &best));
    out
}
