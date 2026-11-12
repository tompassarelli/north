use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

use crate::boxes::{self, Box};
use crate::proc::run;
use crate::step::{self, Reach};
use crate::task::{Task, Tree};
use crate::{cap, classify, map, pickers};

const WORKERS: usize = 8;
const TOP: [usize; 2] = [5, 10];
const CANDIDATE_GREP: usize = 30;
const CANDIDATE_STEP: usize = 15;
const FULL_QUERY: &str = "(a) full grounding query";
const GREP: &str = "(b) text search";
const STEP: &str = "(c) graph step";
const MAP: &str = "(d) classifier, brief + map";
const PACK: &str = "pack as today (Files: line)";
const CANDIDATES: &str = "classifier, brief + search and step candidates";
const STEP_THEN_GREP: &str = "(c) then (b) to fill";
const ORDER: [&str; 7] = [FULL_QUERY, GREP, STEP, MAP, PACK, CANDIDATES, STEP_THEN_GREP];

struct Scored {
    item: u64,
    reachable: bool,
    named: bool,
    picks: BTreeMap<&'static str, Vec<String>>,
    changed: BTreeSet<String>,
}

fn graph_rows(db: &Path, query: &str) -> Result<Value, String> {
    let ran = run("north", &["graph", "ask", "--db", &db.to_string_lossy(), "--json", query], &[], &[], None);
    serde_json::from_slice(&ran.out).map_err(|e| format!("north graph ask: {e}: {}", ran.err.trim()))
}

fn pack_today(text: &str, tree: &Tree) -> Vec<String> {
    let Some(line) = text.lines().find_map(|line| line.trim_start_matches(['-', ' ', '*']).strip_prefix("Files:")) else { return Vec::new() };
    line.split(|c: char| c.is_whitespace() || ",;`()".contains(c)).map(|word| word.trim_end_matches(['.', ':'])).filter(|word| word.contains('/') && tree.has(word)).take(6).map(str::to_owned).collect()
}

fn reply_files(raw: &Path, name: &str, input: &str, north: &str, tree: &Tree) -> Vec<String> {
    let file = raw.join(name);
    let reply = match fs::read_to_string(&file).ok().and_then(|held| serde_json::from_str::<Value>(&held).ok()) {
        Some(reply) => reply,
        None => match classify::ask(north, "claude-haiku-5-5", input) {
            Ok(reply) => {
                let _ = fs::write(&file, reply.to_string());
                reply
            }
            Err(why) => {
                eprintln!("{name}: {why}");
                return Vec::new();
            }
        },
    };
    classify::parse(reply["answer"].as_str().unwrap_or(""), tree).files
}

fn score(found: &Box, repo: &Path, reach_rows: &(Value, Value), raw: &Path, north: &str, classify_on: bool) -> Result<Scored, String> {
    let tree = Tree::at(repo, &found.base)?;
    let reach = Reach::load(&reach_rows.0, &reach_rows.1, &tree);
    let task = Task { id: format!("n{}", found.item), repo: repo.to_path_buf(), base: found.base.clone(), text: found.text.clone(), references: vec![found.commit.clone()] };
    let grep_file = raw.join(format!("n{}.grep.txt", found.item));
    let grep: Vec<String> = match fs::read_to_string(&grep_file) {
        Ok(held) => held.lines().map(str::to_owned).collect(),
        Err(_) => {
            let ranked = pickers::grep_rank(&task)?;
            fs::write(&grep_file, ranked.join("\n")).map_err(|e| format!("{}: {e}", grep_file.display()))?;
            ranked
        }
    };
    let full = pickers::graph_rank(&pickers::graph_output(&task)?, &tree);
    let grounded = step::ground(&found.text, repo, &found.base, &tree, &reach);
    let stepped = grounded.ranked();
    let mut picks: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    if classify_on {
        let symbols = map::symbols_by_path(repo, &found.base, &tree)?;
        let mapped = map::repository_map(&tree, &symbols);
        picks.insert(MAP, reply_files(raw, &format!("n{}.map.json", found.item), &classify::whole_map_input(&found.text, &mapped), north, &tree));
        let none = Vec::new();
        let candidates = cap::union_in_order(&[&grep[..grep.len().min(CANDIDATE_GREP)], &stepped[..stepped.len().min(CANDIDATE_STEP)]]);
        let listed = candidates.iter().map(|path| map::entry(path, symbols.get(path).unwrap_or(&none), 6)).collect::<Vec<_>>().join("\n");
        picks.insert(CANDIDATES, reply_files(raw, &format!("n{}.candidates.json", found.item), &classify::candidates_input(&found.text, &listed), north, &tree));
    }
    picks.insert(STEP_THEN_GREP, cap::union_in_order(&[&stepped, &grep]));
    picks.insert(PACK, pack_today(&found.text, &tree));
    picks.insert(FULL_QUERY, full);
    picks.insert(GREP, grep);
    picks.insert(STEP, stepped);
    let changed: BTreeSet<String> = found.changed.iter().cloned().collect();
    Ok(Scored { item: found.item, reachable: changed.iter().any(|path| tree.has(path)), named: !grounded.named.is_empty(), picks, changed })
}

fn hit(scored: &Scored, picker: &str, top: usize) -> bool {
    scored.picks.get(picker).is_some_and(|files| files.iter().take(top).any(|path| scored.changed.contains(path)))
}

fn share(part: usize, whole: usize) -> String {
    format!("{part} of {whole} ({:.0}%)", 100.0 * part as f64 / whole.max(1) as f64)
}

fn table(out: &mut String, rows: &[&Scored], pickers: &[&str]) {
    let _ = writeln!(out, "| picker | changed file in top 5 | in top 10 | files named, median |\n|---|---|---|---|");
    for picker in pickers {
        let counts: Vec<usize> = TOP.iter().map(|top| rows.iter().filter(|row| hit(row, picker, *top)).count()).collect();
        let mut named: Vec<usize> = rows.iter().map(|row| row.picks.get(picker).map_or(0, |files| files.len().min(10))).collect();
        named.sort_unstable();
        let _ = writeln!(out, "| {picker} | {} | {} | {} |", share(counts[0], rows.len()), share(counts[1], rows.len()), named.get(named.len() / 2).unwrap_or(&0));
    }
}

fn verdict(rows: &[&Scored]) -> String {
    let rate = |picker: &str| 100.0 * rows.iter().filter(|row| hit(row, picker, 5)).count() as f64 / rows.len().max(1) as f64;
    let (grep, best) = (rate(GREP), rate(STEP).max(rate(MAP)));
    let line = if best >= 70.0 && best >= grep + 10.0 {
        "met"
    } else if (best - grep).abs() <= 5.0 {
        "missed, and within 5 points of text search: the registered fail"
    } else {
        "missed"
    };
    format!("Pass line: best of (c) and (d) at top 5 is {best:.0}% against {grep:.0}% for (b); needed at least 70% and at least 10 points above (b). Result: {line}.\n")
}

fn summary(scored: &[Scored], classify_on: bool) -> String {
    let all: Vec<&Scored> = scored.iter().collect();
    let pickers: Vec<&str> = ORDER.iter().copied().filter(|picker| classify_on || (*picker != MAP && *picker != CANDIDATES)).collect();
    let mut out = format!("## G1 result\n\n{} closed north boxes, newest first, each with one landed commit.\n\n", all.len());
    table(&mut out, &all, &pickers);
    out.push('\n');
    out.push_str(&verdict(&all));
    for (title, keep) in [("Boxes whose text names a file tracked at the base", true), ("Boxes whose text names no file", false)] {
        let part: Vec<&Scored> = all.iter().copied().filter(|row| row.named == keep).collect();
        let _ = writeln!(out, "\n{title}: {}\n", part.len());
        table(&mut out, &part, &pickers);
    }
    let reachable = all.iter().filter(|row| row.reachable).count();
    let _ = writeln!(out, "\nCeiling: {} boxes change at least one file that exists at the base.\n\nPer box (item: changed files; top-5 hits as a b c d):", share(reachable, all.len()));
    for row in &all {
        let marks: String = [FULL_QUERY, GREP, STEP, MAP].iter().map(|picker| if hit(row, picker, 5) { '1' } else { '0' }).collect();
        let _ = writeln!(out, "- north#{}: {} changed; {marks}", row.item, row.changed.len());
    }
    out
}

pub fn run_boxes(flags: &BTreeMap<String, String>) -> Result<(), String> {
    let need = |key: &str| flags.get(key).ok_or_else(|| format!("boxes needs --{key}"));
    let work = PathBuf::from(need("work")?);
    let raw = work.join("raw");
    fs::create_dir_all(&raw).map_err(|e| e.to_string())?;
    let repo = PathBuf::from(need("repo")?);
    let db = PathBuf::from(need("graph")?);
    let wanted = flags.get("count").and_then(|count| count.parse().ok()).unwrap_or(100);
    let north = flags.get("north").map_or("north", String::as_str).to_owned();
    let classify_on = flags.get("classifier").is_none_or(|value| value != "off");
    let found = boxes::build(&repo, wanted, &work.join("boxes.json"))?;
    let pairs = graph_rows(&db, r#"{"find":"r","rules":[{"head":{"rel":"r","args":[{"var":"t"},{"var":"p"}]},"body":[{"rel":"triple","args":[{"var":"f"},["keyword","in-file"],{"var":"p"}]},{"rel":"triple","args":[{"var":"t"},["keyword","reaches"],{"var":"f"}]}]}]}"#)?;
    let roots = graph_rows(&db, r#"{"find":"r","rules":[{"head":{"rel":"r","args":[{"var":"c"},{"var":"r"}]},"body":[{"rel":"triple","args":[{"var":"c"},["keyword","root"],{"var":"r"}]}]}]}"#)?;
    let reach_rows = (pairs, roots);
    let queue = Mutex::new((0..found.len()).collect::<Vec<_>>());
    let results = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().unwrap().pop();
                    let Some(index) = next else { break };
                    let outcome = score(&found[index], &repo, &reach_rows, &raw, &north, classify_on);
                    eprintln!("north#{}: {}", found[index].item, if outcome.is_ok() { "scored" } else { "failed" });
                    results.lock().unwrap().push((index, outcome));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(index, _)| *index);
    let scored: Vec<Scored> = results.into_iter().map(|(_, outcome)| outcome).collect::<Result<_, _>>()?;
    let text = summary(&scored, classify_on);
    fs::write(work.join("summary.md"), &text).map_err(|e| e.to_string())?;
    print!("{text}");
    Ok(())
}
