//! `north goals check`: each project's goals, written as named checks with a measured oracle, a
//! bound and a class. A contract is never red, and a landing that turns it red is reverted. A
//! debt-allowed goal may be red up to its ceiling while a `Target-debt:` record names its owner and
//! revisit trigger (docs/rationale/targets.md). A red goal reaches the proxy through the fleet tick.
pub mod attribute;
pub mod ceiling;
pub mod oracle;
pub mod registry;
pub mod tick;
pub mod verdict;

#[cfg(test)]
mod tests;

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use registry::Goal;
use verdict::{Verdict, judge};

const USAGE: &str = "usage: north goals check [--root DIR] [PROJECT...]";
pub const PROJECTS: [&str; 3] = ["north", "makgora", "wisp"];

pub struct Row {
    pub project: String,
    pub goal: Goal,
    pub verdict: Verdict,
}

pub fn code_root() -> PathBuf {
    std::env::var_os("NORTH_CODE_ROOT").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code"), PathBuf::from)
}

fn checkout(code: &Path, project: &str) -> PathBuf {
    code.join(project).join("main")
}

pub fn check(code: &Path, projects: &[String]) -> (Vec<Row>, Vec<String>) {
    let mut wanted: Vec<(String, PathBuf, Goal)> = Vec::new();
    let mut problems = Vec::new();
    for project in projects {
        let root = checkout(code, project);
        let path = root.join(registry::PATH);
        match std::fs::read_to_string(&path).map_err(|why| vec![format!("{}: {why}", path.display())]).and_then(|text| registry::parse(&text).map_err(|found| found.into_iter().map(|why| format!("{project}: {why}")).collect())) {
            Ok(goals) => wanted.extend(goals.into_iter().map(|goal| (project.clone(), root.clone(), goal))),
            Err(found) => problems.extend(found),
        }
    }
    let oracles: Vec<(&Path, &str)> = wanted.iter().map(|(_, root, goal)| (root.as_path(), goal.oracle.as_str())).collect();
    let readings = oracle::measure_all(&oracles);
    let rows = wanted.into_iter().zip(readings).map(|((project, _, goal), reading)| {
        let verdict = judge(&goal, reading);
        Row { project, goal, verdict }
    });
    (rows.collect(), problems)
}

fn number(value: f64, unit: &str) -> String {
    let text = if (value - value.round()).abs() < 1e-9 { format!("{value:.0}") } else { format!("{value:.2}") };
    if unit.is_empty() { text } else { format!("{text} {unit}") }
}

pub fn line(row: &Row) -> String {
    let goal = &row.goal;
    let measured = match &row.verdict {
        Verdict::Green(value) | Verdict::RedContract(value) | Verdict::RedDebt(value) | Verdict::RedOverCeiling(value) => number(*value, &goal.unit),
        Verdict::Unmeasured(why) => why.clone(),
    };
    let class = match &goal.debt {
        Some(debt) => format!("{}, ceiling {}, owner {}, revisit {}", goal.class.word(), number(debt.ceiling, &goal.unit), debt.owner, debt.revisit),
        None => goal.class.word().to_owned(),
    };
    format!("{}/{}: {} {measured} ({} {}); {class}", row.project, goal.name, row.verdict.word(), goal.bound.word(), number(goal.bound.limit(), &goal.unit))
}

pub fn render(rows: &[Row], problems: &[String], seconds: f64) -> String {
    let red = rows.iter().filter(|row| !row.verdict.green()).count();
    let mut out = format!("north goals check: {} goals, {} green, {red} red, in {seconds:.1} s\n", rows.len(), rows.len() - red);
    let mut project = "";
    for row in rows {
        if row.project != project {
            project = &row.project;
            let _ = writeln!(out, "\n{project}");
        }
        let _ = writeln!(out, "- {}", line(row).trim_start_matches(&format!("{}/", row.project)));
        let _ = writeln!(out, "  source: {}", row.goal.source);
        let _ = writeln!(out, "  oracle: {}", row.goal.oracle);
    }
    problems.iter().for_each(|problem| {
        let _ = writeln!(out, "\nnot read: {problem}");
    });
    out
}

pub fn failing(rows: &[Row], problems: &[String]) -> bool {
    !problems.is_empty() || rows.iter().any(|row| row.verdict.failing())
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(rest) = arguments.split_first().filter(|(verb, _)| verb.as_str() == "check").map(|(_, rest)| rest) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let mut code = code_root();
    let mut projects: Vec<String> = Vec::new();
    let mut rest = rest.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--root" => match rest.next() {
                Some(root) => code = PathBuf::from(root),
                None => {
                    eprintln!("{USAGE}");
                    return 2;
                }
            },
            name if PROJECTS.contains(&name) => projects.push(name.to_owned()),
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    if projects.is_empty() {
        projects = PROJECTS.iter().map(|name| (*name).to_owned()).collect();
    }
    let began = Instant::now();
    let (rows, problems) = check(&code, &projects);
    print!("{}", render(&rows, &problems, began.elapsed().as_secs_f64()));
    u8::from(failing(&rows, &problems))
}
