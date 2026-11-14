//! `north docs readme`: the generated blocks of the front pages. The README's quickstart is rendered from
//! the numbered steps of `docs/onboarding.md`, the comparison page's table from the rows in
//! `docs/readme/comparison.toml`, and ARCHITECTURE.md's map from `docs/readme/layout.toml`, which must
//! name every top-level directory and crate family of the tree and nothing else. Text outside the blocks
//! is each page's own and is kept as written.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use toml_edit::{DocumentMut, Item, Table};

use crate::bootstrap::{self, Samples, median};
use crate::registry::{files, read};

type Block = fn(&Path) -> Result<String, String>;

pub fn readme(source: &Path) -> Result<String, String> {
    fill(source, "README.md", &[("quickstart", quickstart)])
}

pub fn comparison_page(source: &Path) -> Result<String, String> {
    fill(source, "docs/rationale/plain-runner-comparison.md", &[("comparison", comparison)])
}

pub fn architecture(source: &Path) -> Result<String, String> {
    fill(source, "ARCHITECTURE.md", &[("layout", layout)])
}

fn fill(source: &Path, page: &str, blocks: &[(&str, Block)]) -> Result<String, String> {
    let root = source.parent().and_then(Path::parent).ok_or("docs/readme sits in no repository")?;
    let mut text = read(&root.join(page))?;
    for &(name, block) in blocks {
        let open = format!("<!-- north docs readme: {name} -->\n");
        let close = format!("<!-- end {name} -->");
        let start = text.find(&open).ok_or_else(|| format!("{page}: no `{}` line", open.trim_end()))? + open.len();
        let end = start + text[start..].find(&close).ok_or_else(|| format!("{page}: no `{close}` after the {name} block"))?;
        text.replace_range(start..end, &block(root)?);
    }
    Ok(text)
}

fn document(path: &Path) -> Result<DocumentMut, String> {
    read(path)?.parse().map_err(|error| format!("{}: {error}", path.display()))
}

fn text<'a>(table: &'a Table, key: &str, path: &Path) -> Result<&'a str, String> {
    table.get(key).and_then(Item::as_str).ok_or_else(|| format!("{}: `{key}` is missing or not text", path.display()))
}

struct Row {
    run: i64,
    level: i64,
    arm: String,
    task: String,
    landed: bool,
    seconds: f64,
    tokens: f64,
    cost: f64,
}

fn rows(doc: &DocumentMut, path: &Path) -> Result<Vec<Row>, String> {
    let bad = |field: &str| format!("{}: a row has no `{field}`", path.display());
    let array = doc.get("rows").and_then(Item::as_array).ok_or_else(|| format!("{}: no `rows` array", path.display()))?;
    array
        .iter()
        .map(|value| {
            let row = value.as_inline_table().ok_or_else(|| format!("{}: a row is not an inline table", path.display()))?;
            let number = |key: &str| row.get(key).and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))).ok_or_else(|| bad(key));
            let word = |key: &str| row.get(key).and_then(|v| v.as_str()).map(str::to_owned).ok_or_else(|| bad(key));
            Ok(Row {
                run: row.get("run").and_then(|v| v.as_integer()).ok_or_else(|| bad("run"))?,
                level: row.get("level").and_then(|v| v.as_integer()).ok_or_else(|| bad("level"))?,
                arm: word("arm")?,
                task: word("task")?,
                landed: row.get("landed").and_then(|v| v.as_bool()).ok_or_else(|| bad("landed"))?,
                seconds: number("seconds")?,
                tokens: number("tokens")?,
                cost: number("cost_usd")?,
            })
        })
        .collect()
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn landed_times(rows: &[&Row], arm: &str) -> Samples {
    let mut by_task = Samples::new();
    for row in rows.iter().filter(|row| row.arm == arm && row.landed) {
        by_task.entry(row.task.clone()).or_default().push(row.seconds);
    }
    by_task
}

fn task_means(rows: &[&Row], arm: &str) -> BTreeMap<String, f64> {
    landed_times(rows, arm).into_iter().map(|(task, seconds)| (task, mean(&seconds))).collect()
}

struct Reading {
    interval: Option<(f64, f64)>,
    band: Option<f64>,
    label: String,
}

fn reading(rows: &[&Row], base: &str, rival: &str, runs: usize, ratio: f64, seed: u32) -> Reading {
    let mut order: Vec<&str> = Vec::new();
    for row in rows.iter().filter(|row| row.arm == base && row.landed) {
        if !order.contains(&row.task.as_str()) {
            order.push(&row.task);
        }
    }
    let (mine, theirs) = (landed_times(rows, base), landed_times(rows, rival));
    let interval = bootstrap::interval(&order, &mine, &theirs, seed);
    let band = if runs >= 3 { bootstrap::band(&order, &theirs) } else { None };
    let label = match (band, interval, bootstrap::drift(&order, &theirs)) {
        _ if runs < 3 => format!("undecided: {runs} valid run{}, the band needs three", if runs == 1 { "" } else { "s" }),
        (_, _, Some(way)) => format!("not read: each run {way} than the last on every task"),
        (Some(band), Some(interval), None) => bootstrap::label(ratio, interval, band),
        _ => "not read: a task has no landed time in both arms".to_owned(),
    };
    Reading { interval, band, label }
}

struct Cell {
    runs: usize,
    tasks: usize,
    landed: Vec<String>,
    median: f64,
    rival: String,
    rival_median: f64,
    by_run: Vec<f64>,
    ratio: f64,
    cost: f64,
    tokens: f64,
    reading: Reading,
}

fn of<'a>(rows: &[&'a Row], arm: &str) -> Vec<&'a Row> {
    rows.iter().filter(|row| row.arm == arm).copied().collect()
}

fn landed_median(rows: &[&Row], arm: &str) -> f64 {
    median(of(rows, arm).iter().filter(|row| row.landed).map(|row| row.seconds).collect())
}

/// The arm with the lower median time among `rivals`, and north's paired ratio against it.
fn paired<'a>(rows: &[&Row], base: &str, rivals: &[(&'a str, &'a str)]) -> Option<(&'a str, &'a str, f64)> {
    let &(rival, name) = rivals.iter().min_by(|a, b| landed_median(rows, a.0).total_cmp(&landed_median(rows, b.0)))?;
    let theirs = task_means(rows, rival);
    Some((rival, name, median(task_means(rows, base).iter().filter_map(|(task, seconds)| Some(seconds / theirs.get(task)?)).collect())))
}

fn cell(rows: &[&Row], base: &str, rivals: &[(&str, &str)], seed: u32) -> Option<Cell> {
    let per_task = |arm: &str, pick: fn(&Row) -> f64| mean(&of(rows, arm).iter().map(|row| pick(row)).collect::<Vec<f64>>());
    let (rival, rival_name, ratio) = paired(rows, base, rivals)?;
    let mine = of(rows, base);
    let runs: BTreeSet<i64> = mine.iter().map(|row| row.run).collect();
    let by_run = runs.iter().filter_map(|run| paired(&rows.iter().filter(|row| row.run == *run).copied().collect::<Vec<_>>(), base, rivals).map(|found| found.2)).collect();
    Some(Cell {
        runs: runs.len(),
        tasks: mine.iter().map(|row| row.task.as_str()).collect::<BTreeSet<_>>().len(),
        landed: std::iter::once(base).chain(rivals.iter().map(|(arm, _)| *arm)).map(|arm| match of(rows, arm) { ran if ran.is_empty() => "not run".to_owned(), ran => format!("{} of {}", ran.iter().filter(|row| row.landed).count(), ran.len()) }).collect(),
        median: landed_median(rows, base),
        rival: rival_name.to_owned(),
        rival_median: landed_median(rows, rival),
        by_run,
        ratio,
        cost: per_task(base, |row| row.cost) / per_task(rival, |row| row.cost),
        tokens: per_task(rival, |row| row.tokens) / per_task(base, |row| row.tokens),
        reading: reading(rows, base, rival, runs.len(), ratio, seed),
    })
}

fn list(doc: &DocumentMut, key: &str, path: &Path) -> Result<String, String> {
    let items = doc.get(key).and_then(Item::as_array).ok_or_else(|| format!("{}: no `{key}` list", path.display()))?;
    items.iter().map(|value| value.as_str().map(|item| format!("- {item}\n")).ok_or_else(|| format!("{}: `{key}` holds an entry that is not text: {value:?}", path.display()))).collect()
}

fn comparison(root: &Path) -> Result<String, String> {
    let path = root.join("docs/readme/comparison.toml");
    let doc = document(&path)?;
    let field = |key: &str| text(doc.as_table(), key, &path);
    let table = |key: &str| doc.get(key).and_then(Item::as_table).ok_or_else(|| format!("{}: no `[{key}]` table", path.display()));
    let names: Vec<(&str, &str)> = table("arms")?.iter().filter_map(|(key, item)| Some((key, item.as_str()?))).collect();
    let Some((&(base, _), rivals)) = names.split_first().filter(|(_, rivals)| !rivals.is_empty()) else {
        return Err(format!("{}: `[arms]` needs north first and at least one arm to compare with", path.display()));
    };
    let rows = rows(&doc, &path)?;
    if let Some(row) = rows.iter().find(|row| !names.iter().any(|(arm, _)| *arm == row.arm)) {
        return Err(format!("{}: row arm `{}` is not in `[arms]`", path.display(), row.arm));
    }
    let cells = table("cells")?;
    let seed = doc.get("seed").and_then(Item::as_integer).and_then(|seed| u32::try_from(seed).ok()).ok_or_else(|| format!("{}: `seed` is missing or not a 32-bit count", path.display()))?;
    let rival = field("rival")?;
    let mut out = format!(
        "Measured {} against {} on {}, with {}; {}. Time runs {}. Only valid runs are counted.\n\n| Cell | Valid runs x tasks | Landed: {} | north, median | Faster {rival} arm, median | Time ratio, each run | Time ratio R, all valid runs (80% interval) | {rival} band N | Cost ratio | Tokens | Reading |\n|---|---|---|---|---|---|---|---|---|---|---|\n",
        field("date")?,
        field("competitor")?,
        field("tasks")?,
        field("model")?,
        field("host")?,
        field("clock")?,
        names.iter().map(|(_, name)| *name).collect::<Vec<_>>().join(" / "),
    );
    for level in rows.iter().map(|row| row.level).collect::<BTreeSet<i64>>() {
        let name = cells.get(&level.to_string()).and_then(Item::as_str).ok_or_else(|| format!("{}: `[cells]` names no cell for level {level}", path.display()))?;
        let at: Vec<&Row> = rows.iter().filter(|row| row.level == level).collect();
        let c = cell(&at, base, rivals, seed).ok_or_else(|| format!("{}: the {name} has no rows to compare", path.display()))?;
        let by_run = c.by_run.iter().map(|ratio| format!("{ratio:.2}")).collect::<Vec<_>>().join(", ");
        let ratio = match c.reading.interval {
            Some((low, high)) => format!("{:.3} ({low:.3} to {high:.3})", c.ratio),
            None => format!("{:.3}", c.ratio),
        };
        let band = c.reading.band.map_or_else(|| "none".to_owned(), |band| format!("{band:.3}"));
        out.push_str(&format!(
            "| {name} | {} x {} | {} | {} s | {}, {} s | {by_run} | {ratio} | {band} | {:.2} | {:.0}x fewer | {} |\n",
            c.runs, c.tasks, c.landed.join(" / "), c.median, c.rival, c.rival_median, c.cost, c.tokens, c.reading.label
        ));
    }
    out.push_str(&format!(
        "\nTime ratio: the median over tasks of north's mean time divided by the faster arm's, below 1 when north is faster; the faster arm is the one with the lower median in that cell, or in that run. R's 80% interval resamples tasks and then runs within each task, 10,000 times from seed {seed}. Band N: the median over tasks of the faster {rival} arm's slowest time over its fastest across the cell's runs, read from three valid runs. Reading, on time: far ahead when R is at or below 0.67 and below 1/N, ahead when R is below 1/N, behind when R is above N, inside the band otherwise, with low confidence when the interval crosses the threshold the label is read against; a cell whose every task got slower, or faster, run after run is not read. Cost ratio: north's cost per task over the faster arm's, at the {}.\n\nNot counted:\n\n{}\nNot measured yet:\n\n{}",
        field("price")?,
        list(&doc, "void", &path)?,
        list(&doc, "open", &path)?
    ));
    Ok(out)
}

fn numbered(heading: &str) -> bool {
    heading.split_once(". ").is_some_and(|(number, _)| number.parse::<u32>().is_ok())
}

fn quickstart(root: &Path) -> Result<String, String> {
    let path = root.join("docs/onboarding.md");
    let page = read(&path)?;
    let mut commands = String::new();
    let mut step = false;
    let mut block = false;
    for line in page.lines() {
        match line {
            _ if line.starts_with("## ") => step = numbered(&line[3..]),
            "```sh" if step => block = true,
            "```" => block = false,
            _ if block => commands.push_str(&format!("{line}\n")),
            _ => {}
        }
    }
    if commands.is_empty() {
        return Err(format!("{}: no `sh` block under a numbered step", path.display()));
    }
    Ok(format!("```sh\n{commands}```\n"))
}

fn top_dirs(root: &Path) -> Result<BTreeSet<String>, String> {
    let output = Command::new("git").arg("-C").arg(root).args(["ls-files", "-z"]).output().map_err(|error| format!("git ls-files: {error}"))?;
    if !output.status.success() {
        return Err(format!("git ls-files in {}: {}", root.display(), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter_map(|path| std::str::from_utf8(path).ok()?.split_once('/').map(|(dir, _)| dir.to_owned()))
        .filter(|dir| !dir.starts_with('.'))
        .collect())
}

struct Placed {
    name: String,
    dir: String,
}

fn crate_families(root: &Path) -> Result<BTreeMap<(i64, String), Vec<Placed>>, String> {
    let mut families: BTreeMap<(i64, String), Vec<Placed>> = BTreeMap::new();
    for path in files(&root.join("structure"), "toml")? {
        let doc = document(&path)?;
        let entries = doc.get("crate_dag").and_then(|dag| dag.get("crate")).and_then(Item::as_array_of_tables);
        for entry in entries.into_iter().flat_map(|array| array.iter()) {
            let Some(layer) = entry.get("layer").and_then(Item::as_integer) else { continue };
            let name = text(entry, "name", &path)?.to_owned();
            let dir = entry
                .get("paths")
                .and_then(Item::as_array)
                .and_then(|paths| paths.iter().next())
                .and_then(|first| first.as_str())
                .and_then(|first| first.split("/src/").next())
                .ok_or_else(|| format!("{}: crate {name} has no `paths`", path.display()))?
                .to_owned();
            let family = entry.get("family").and_then(Item::as_str).unwrap_or_default().to_owned();
            families.entry((layer, family)).or_default().push(Placed { name, dir });
        }
    }
    Ok(families)
}

fn layout(root: &Path) -> Result<String, String> {
    let path = root.join("docs/readme/layout.toml");
    let doc = document(&path)?;
    let dirs = doc.get("dirs").and_then(Item::as_table).ok_or_else(|| format!("{}: no `[dirs]` table", path.display()))?;
    let tree = top_dirs(root)?;
    let listed: BTreeSet<String> = dirs.iter().map(|(dir, _)| dir.to_owned()).collect();
    let mut faults: Vec<String> = tree.difference(&listed).map(|dir| format!("top-level directory {dir}/ has no line in [dirs]")).collect();
    faults.extend(listed.difference(&tree).map(|dir| format!("[dirs] names {dir}/, which the tree does not have")));
    let mut out = String::new();
    for (dir, purpose) in dirs.iter() {
        out.push_str(&format!("- [{dir}/]({dir}/): {}\n", purpose.as_str().ok_or_else(|| format!("{}: [dirs] {dir} is not text", path.display()))?));
    }
    let mut placed = crate_families(root)?;
    let crate_dirs: BTreeSet<String> = std::fs::read_dir(root.join("crates"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().join("Cargo.toml").is_file())
        .map(|entry| format!("crates/{}", entry.file_name().to_string_lossy()))
        .collect();
    let placed_dirs: BTreeSet<String> = placed.values().flatten().map(|krate| krate.dir.split('/').take(2).collect::<Vec<_>>().join("/")).collect();
    faults.extend(crate_dirs.difference(&placed_dirs).map(|dir| format!("{dir} has no layer in structure/")));
    out.push_str("\nThe crates under [crates/](crates/), by the layer each sits in; a crate depends only on layers below it, and [the crates table](#the-crates) gives each crate's purpose:\n\n");
    let families = doc.get("family").and_then(Item::as_array_of_tables).ok_or_else(|| format!("{}: no `[[family]]` tables", path.display()))?;
    for family in families.iter() {
        let layer = family.get("layer").and_then(Item::as_integer).ok_or_else(|| format!("{}: a [[family]] has no `layer`", path.display()))?;
        let name = text(family, "name", &path)?;
        let key = (layer, family.get("family").and_then(Item::as_str).unwrap_or_default().to_owned());
        let Some(mut crates) = placed.remove(&key) else {
            faults.push(format!("[[family]] {layer} {name} has no crate in structure/"));
            continue;
        };
        crates.sort_by(|a, b| a.name.cmp(&b.name));
        let members = crates.iter().map(|krate| format!("[{}]({}/)", krate.name, krate.dir)).collect::<Vec<_>>().join(", ");
        out.push_str(&format!("- **{layer} {name}**, {}: {members}\n", text(family, "purpose", &path)?));
    }
    faults.extend(placed.keys().map(|(layer, family)| format!("layer {layer} family `{family}` in structure/ has no [[family]] in docs/readme/layout.toml")));
    if faults.is_empty() { Ok(out) } else { Err(format!("README layout and the tree disagree:\n  {}", faults.join("\n  "))) }
}
