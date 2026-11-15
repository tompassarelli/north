use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use toml_edit::DocumentMut;

use crate::registry::read;

const RUNS_WANTED: usize = 3;
const TASKS: usize = 5;
const KEY: &str = "large_rows";

const INTRO: &str = "The registered large cells, from the five frozen large tasks of north's own history whose checks run through Buck2 remote execution (north#1429), one session per task, both arms on one build host: five at once, then one at a time. A run is valid when both arms hold all five tasks, and a cell is read only at three valid runs.\n\n";

const HEADER: &str = "| Cell | Valid runs x tasks | Correct: north / plain | Time to all five landed: north / plain | F1, the wait for all five | F2, per task | F3, first try | Cost per run: north / plain | Reading |\n|---|---|---|---|---|---|---|---|---|\n";

const LEGEND: &str = "\nF1 (five at once only): north's time to all five landed is at most 0.8 of plain's and north has at least as many correct. F2: north is at least 20 percent faster on at least 3 of the 5 tasks and more than 10 percent slower on none, counting only tasks both arms got correct in every run. F3: north's first hand-ins that passed are at least plain's. Each line reads each task's mean over the valid runs; the time to all five landed is the arm's last landing at five at once and the five times added at one worker, and a unit that reached the 4-hour stop, landed nothing or failed its check is a failed row, not a time. Cost sits beside the lines and decides none; a lead bought at more than twice plain's cost says so. A cell with fewer than three valid runs shows only its run count and no figures.\n\n";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Five,
    One,
}

impl Cell {
    const BOTH: [Cell; 2] = [Cell::Five, Cell::One];

    fn id(self) -> &'static str {
        match self {
            Self::Five => "L5",
            Self::One => "L1",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Five => "large five-at-once cell",
            Self::One => "large one-worker cell",
        }
    }

    fn runs(self) -> [&'static str; RUNS_WANTED] {
        match self {
            Self::Five => ["lgb5-1115a", "lgb5-1115b", "lgb5-1115c"],
            Self::One => ["lgb1-1115a", "lgb1-1115b", "lgb1-1115c"],
        }
    }

    fn of_id(id: &str) -> Option<Self> {
        Self::BOTH.into_iter().find(|cell| cell.id() == id)
    }

    fn of_run(run: &str) -> Option<Self> {
        Self::BOTH.into_iter().find(|cell| cell.runs().contains(&run))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Arm {
    North,
    Plain,
}

impl Arm {
    fn id(self) -> &'static str {
        match self {
            Self::North => "north",
            Self::Plain => "plain",
        }
    }

    fn of_id(id: &str) -> Option<Self> {
        [Self::North, Self::Plain].into_iter().find(|arm| arm.id() == id)
    }

    fn of_label(label: &str) -> Option<(Cell, Self)> {
        match label {
            "north-5-on-one-host" => Some((Cell::Five, Self::North)),
            "plain-5-on-one-host" => Some((Cell::Five, Self::Plain)),
            "north-1" => Some((Cell::One, Self::North)),
            "plain-1" => Some((Cell::One, Self::Plain)),
            _ => None,
        }
    }
}

struct Row {
    run: String,
    cell: Cell,
    arm: Arm,
    task: String,
    correct: bool,
    seconds: f64,
    first_handin_pass: bool,
    failed_handins: i64,
    tokens: i64,
    cost_usd: f64,
}

impl Row {
    fn line(&self) -> String {
        format!(
            "  {{ run = {:?}, cell = {:?}, arm = {:?}, task = {:?}, correct = {}, seconds = {}, first_handin_pass = {}, failed_handins = {}, tokens = {}, cost_usd = {} }},\n",
            self.run,
            self.cell.id(),
            self.arm.id(),
            self.task,
            self.correct,
            self.seconds,
            self.first_handin_pass,
            self.failed_handins,
            self.tokens,
            self.cost_usd
        )
    }

    fn same_unit(&self, other: &Row) -> bool {
        self.run == other.run && self.arm == other.arm && self.task == other.task
    }
}

fn stored(doc: &DocumentMut, path: &Path) -> Result<Vec<Row>, String> {
    let Some(item) = doc.get(KEY) else { return Ok(Vec::new()) };
    let array = item.as_array().ok_or_else(|| format!("{}: `{KEY}` is not an array", path.display()))?;
    let bad = |field: &str| format!("{}: a `{KEY}` row has no `{field}`", path.display());
    array
        .iter()
        .map(|value| {
            let row = value.as_inline_table().ok_or_else(|| format!("{}: a `{KEY}` row is not an inline table", path.display()))?;
            let word = |key: &str| row.get(key).and_then(|v| v.as_str()).ok_or_else(|| bad(key));
            let flag = |key: &str| row.get(key).and_then(|v| v.as_bool()).ok_or_else(|| bad(key));
            let number = |key: &str| row.get(key).and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))).ok_or_else(|| bad(key));
            Ok(Row {
                run: word("run")?.to_owned(),
                cell: Cell::of_id(word("cell")?).ok_or_else(|| bad("cell"))?,
                arm: Arm::of_id(word("arm")?).ok_or_else(|| bad("arm"))?,
                task: word("task")?.to_owned(),
                correct: flag("correct")?,
                seconds: number("seconds")?,
                first_handin_pass: flag("first_handin_pass")?,
                failed_handins: number("failed_handins")? as i64,
                tokens: number("tokens")? as i64,
                cost_usd: number("cost_usd")?,
            })
        })
        .collect()
}

enum Fetched {
    Kept(Row),
    Skipped(String),
}

fn run_of_file(file: &Path) -> Option<String> {
    let name = file.file_name()?.to_str()?;
    let stem = name.strip_prefix("results-")?.split(".jsonl").next()?;
    Cell::BOTH.into_iter().flat_map(Cell::runs).find(|run| stem == *run || stem.starts_with(&format!("{run}-"))).map(str::to_owned)
}

fn fetch(value: &Value, file: &Path) -> Result<Fetched, String> {
    let fault = |key: &str| format!("{}: a row has no usable `{key}`", file.display());
    let field = |key: &str| value.get(key).filter(|found| !found.is_null()).ok_or_else(|| fault(key));
    let flag = |key: &str| value.get(key).and_then(Value::as_bool).unwrap_or(false);
    let Some(run) = value.get("run_prefix").and_then(Value::as_str).map(str::to_owned).or_else(|| run_of_file(file)) else {
        return Ok(Fetched::Skipped("no run".to_owned()));
    };
    let Some(cell) = Cell::of_run(&run) else { return Ok(Fetched::Skipped(run)) };
    let label = field("arm_label")?.as_str().ok_or_else(|| fault("arm_label"))?;
    let Some((labelled, arm)) = Arm::of_label(label) else { return Ok(Fetched::Skipped(label.to_owned())) };
    if labelled != cell {
        return Err(format!("{}: a row of {run} is labelled {label}", file.display()));
    }
    let tokens = field("tokens")?;
    Ok(Fetched::Kept(Row {
        cell,
        arm,
        task: field("task")?.as_str().ok_or_else(|| fault("task"))?.to_owned(),
        correct: flag("merged") && flag("acceptance") && !flag("abandoned"),
        seconds: field("t_merge_s")?.as_f64().ok_or_else(|| fault("t_merge_s"))?,
        first_handin_pass: flag("first_handin_pass"),
        failed_handins: value.get("failed_handins").and_then(Value::as_i64).unwrap_or(0),
        tokens: ["input", "output", "cache_read", "cache_write"].iter().map(|kind| tokens.get(kind).and_then(Value::as_i64).unwrap_or(0)).sum(),
        cost_usd: field("cost_usd")?.as_f64().ok_or_else(|| fault("cost_usd"))?,
        run,
    }))
}

fn with_rows(text: &str, rows: &[Row]) -> Result<String, String> {
    let block = format!("{KEY} = [\n{}]\n", rows.iter().map(Row::line).collect::<String>());
    let mut updated = text.to_owned();
    if let Some(at) = text.find(&format!("\n{KEY} = [\n")) {
        let from = at + 1;
        let end = from + text[from..].find("\n]\n").ok_or_else(|| format!("`{KEY}` has no closing bracket line"))? + 3;
        updated.replace_range(from..end, &block);
    } else {
        let at = text.find("\n[").ok_or("no table follows the rows to place `large_rows` before")? + 1;
        updated.insert_str(at, &format!("{block}\n"));
    }
    Ok(updated)
}

fn absorb(file: &Path, rows: &mut Vec<Row>) -> Result<String, String> {
    let (mut kept, mut skipped) = (0, BTreeSet::new());
    for line in read(file)?.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).map_err(|error| format!("{}: {error}", file.display()))?;
        match fetch(&value, file)? {
            Fetched::Kept(row) => {
                kept += 1;
                match rows.iter_mut().find(|have| have.same_unit(&row)) {
                    Some(have) => *have = row,
                    None => rows.push(row),
                }
            }
            Fetched::Skipped(what) => {
                skipped.insert(what);
            }
        }
    }
    let skipped = skipped.into_iter().collect::<Vec<_>>().join(", ");
    let note = if skipped.is_empty() { String::new() } else { format!("; skipped rows of {skipped}, not one of the six registered runs") };
    Ok(format!("{}: {kept} rows kept{note}", file.display()))
}

pub fn import(path: &Path, files: &[PathBuf]) -> Result<Vec<String>, String> {
    let text = read(path)?;
    let doc: DocumentMut = text.parse().map_err(|error| format!("{}: {error}", path.display()))?;
    let mut rows = stored(&doc, path)?;
    let notes = files.iter().map(|file| absorb(file, &mut rows)).collect::<Result<Vec<_>, _>>()?;
    rows.sort_by(|a, b| (&a.run, a.arm, &a.task).cmp(&(&b.run, b.arm, &b.task)));
    let updated = with_rows(&text, &rows)?;
    if updated != text {
        fs::write(path, updated).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(notes)
}

pub fn run_import(arguments: &[String]) -> Result<(), String> {
    let usage = "usage: north docs readme import-large [--root DIR] FILE...";
    let mut root = None;
    let mut files = Vec::new();
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--root" => root = Some(PathBuf::from(rest.next().ok_or(usage)?)),
            _ => files.push(PathBuf::from(argument)),
        }
    }
    if files.is_empty() {
        return Err(usage.to_owned());
    }
    let found = || std::env::current_dir().ok()?.ancestors().find(|dir| dir.join("docs/readme/comparison.toml").is_file()).map(Path::to_path_buf);
    let root = root.or_else(found).ok_or("no docs/readme/comparison.toml found from the current directory")?;
    for note in import(&root.join("docs/readme/comparison.toml"), &files)? {
        println!("{note}");
    }
    Ok(())
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn span(seconds: Option<f64>) -> String {
    match seconds {
        None => "not all landed".to_owned(),
        Some(s) if s < 120.0 => format!("{s:.0} s"),
        Some(s) if s < 7200.0 => format!("{:.0} min", s / 60.0),
        Some(s) => format!("{:.1} h", s / 3600.0),
    }
}

struct Line {
    name: &'static str,
    passed: bool,
    detail: String,
}

impl Line {
    fn show(&self) -> String {
        if self.passed { format!("pass: {}", self.detail) } else { format!("loss: {}", self.detail) }
    }
}

fn runs_of<'a>(rows: &[&'a Row]) -> Vec<Vec<&'a Row>> {
    let mut by_run: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for row in rows {
        by_run.entry(&row.run).or_default().push(row);
    }
    let landed = |run: &[&Row], arm: Arm| run.iter().filter(|row| row.arm == arm).map(|row| &row.task).collect::<BTreeSet<_>>().len() == TASKS;
    by_run.into_values().filter(|run| landed(run, Arm::North) && landed(run, Arm::Plain)).collect()
}

fn time_to_all(runs: &[Vec<&Row>], cell: Cell, arm: Arm) -> Option<f64> {
    let per_run: Option<Vec<f64>> = runs
        .iter()
        .map(|run| {
            let units: Vec<&&Row> = run.iter().filter(|row| row.arm == arm).collect();
            let all_correct = units.iter().all(|row| row.correct);
            all_correct.then(|| match cell {
                Cell::Five => units.iter().map(|row| row.seconds).fold(0.0, f64::max),
                Cell::One => units.iter().map(|row| row.seconds).sum(),
            })
        })
        .collect();
    per_run.map(|times| mean(&times))
}

fn count(rows: &[&Row], pick: fn(&Row) -> bool) -> usize {
    rows.iter().filter(|row| pick(row)).count()
}

fn wait_line(north: Option<f64>, plain: Option<f64>, north_correct: usize, plain_correct: usize, total: usize) -> Line {
    let (wait, on_time) = match (north, plain) {
        (Some(n), Some(p)) => (format!("{:.2} of plain's time", n / p), n <= 0.8 * p),
        (Some(_), None) => ("plain did not land all five in every run".to_owned(), true),
        (None, _) => ("north did not land all five in every run".to_owned(), false),
    };
    Line { name: "F1", passed: on_time && north_correct >= plain_correct, detail: format!("{wait}, {north_correct} against {plain_correct} correct of {total}") }
}

fn of_task<'a>(rows: &[&'a Row], task: &str) -> Vec<&'a Row> {
    rows.iter().copied().filter(|row| row.task == task).collect()
}

fn task_line(north: &[&Row], plain: &[&Row]) -> Line {
    let (mut counted, mut faster, mut slower) = (0, 0, 0);
    let tasks: BTreeSet<&str> = north.iter().map(|row| row.task.as_str()).collect();
    for task in tasks {
        let (n, p) = (of_task(north, task), of_task(plain, task));
        if n.iter().chain(&p).all(|row| row.correct) {
            let seconds = |rows: &[&Row]| mean(&rows.iter().map(|row| row.seconds).collect::<Vec<_>>());
            let (n, p) = (seconds(&n), seconds(&p));
            counted += 1;
            faster += usize::from(n <= 0.8 * p);
            slower += usize::from(n > 1.1 * p);
        }
    }
    Line { name: "F2", passed: faster >= 3 && slower == 0, detail: format!("{faster} of {TASKS} tasks 20% faster, {slower} more than 10% slower, {counted} counted") }
}

fn first_try_line(north: &[&Row], plain: &[&Row]) -> Line {
    let (n, p) = (count(north, |row| row.first_handin_pass), count(plain, |row| row.first_handin_pass));
    let failed = |rows: &[&Row]| rows.iter().map(|row| row.failed_handins).sum::<i64>();
    Line { name: "F3", passed: n >= p, detail: format!("first hand-in passed {n} against {p} of {}, failed hand-ins {} against {}", north.len(), failed(north), failed(plain)) }
}

fn reading(lines: &[Line], cost_ratio: f64) -> String {
    let names = |passed: bool| lines.iter().filter(|line| line.passed == passed).map(|line| line.name).collect::<Vec<_>>().join(", ");
    let (won, lost) = (names(true), names(false));
    let mut parts = Vec::new();
    if !won.is_empty() {
        parts.push(format!("passes {won}"));
    }
    if !lost.is_empty() {
        parts.push(format!("loses {lost}"));
    }
    if !won.is_empty() && cost_ratio > 2.0 {
        parts.push(format!("the lead cost {cost_ratio:.1}x plain's"));
    }
    parts.join("; ")
}

fn cell_row(cell: Cell, rows: &[&Row]) -> String {
    let runs = runs_of(rows);
    let valid = runs.len();
    if valid == 0 {
        return format!("| {} | 0 x {TASKS} | - | - | - | - | - | - | not yet run: 0 of {RUNS_WANTED} runs |\n", cell.name());
    }
    if valid < RUNS_WANTED {
        return format!("| {} | {valid} x {TASKS} | - | - | - | - | - | - | {valid} of {RUNS_WANTED} runs |\n", cell.name());
    }
    let of = |arm: Arm| runs.iter().flatten().copied().filter(|row| row.arm == arm).collect::<Vec<&Row>>();
    let (north, plain) = (of(Arm::North), of(Arm::Plain));
    let (n_correct, p_correct) = (count(&north, |row| row.correct), count(&plain, |row| row.correct));
    let (n_time, p_time) = (time_to_all(&runs, cell, Arm::North), time_to_all(&runs, cell, Arm::Plain));
    let cost = |rows: &[&Row]| rows.iter().map(|row| row.cost_usd).sum::<f64>();
    let (n_cost, p_cost) = (cost(&north), cost(&plain));
    let lines = [
        (cell == Cell::Five).then(|| wait_line(n_time, p_time, n_correct, p_correct, north.len())),
        Some(task_line(&north, &plain)),
        Some(first_try_line(&north, &plain)),
    ];
    let shown = |name: &str| lines.iter().flatten().find(|line| line.name == name).map_or_else(|| "five at once only".to_owned(), |line| line.show());
    let (wait, task, first) = (shown("F1"), shown("F2"), shown("F3"));
    let applicable: Vec<Line> = lines.into_iter().flatten().collect();
    format!(
        "| {} | {valid} x {TASKS} | {n_correct} of {} / {p_correct} of {} | {} / {} | {wait} | {task} | {first} | ${:.2} / ${:.2}, {:.2}x | {} |\n",
        cell.name(),
        north.len(),
        plain.len(),
        span(n_time),
        span(p_time),
        n_cost / valid as f64,
        p_cost / valid as f64,
        n_cost / p_cost,
        reading(&applicable, n_cost / p_cost)
    )
}

pub fn section(doc: &DocumentMut, path: &Path) -> Result<String, String> {
    let rows = stored(doc, path)?;
    let mut out = format!("{INTRO}{HEADER}");
    for cell in Cell::BOTH {
        let own: Vec<&Row> = rows.iter().filter(|row| row.cell == cell).collect();
        out.push_str(&cell_row(cell, &own));
    }
    out.push_str(LEGEND);
    Ok(out)
}
