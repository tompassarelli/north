//! `north check concerns`: each project's concerns, written as named checks with a measured oracle, a
//! bound and a class. A contract is never red, and a landing that turns it red is reverted. A
//! debt-allowed concern may be red up to its ceiling while a `Target-debt:` record names its owner and
//! revisit trigger (docs/rationale/targets.md). A red concern reaches the proxy through the fleet tick.
pub mod attribute;
pub mod ceiling;
pub mod history;
pub mod oracle;
pub mod registry;
pub mod tick;
pub mod trend;
pub mod verdict;

#[cfg(test)]
mod tests;

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use history::{Recorded, Store};
use north_base::foundation::clock::Timestamp;
use registry::Concern;
use verdict::{Verdict, judge};

const USAGE: &str = "usage: north check concerns [--root DIR] [--trend] [PROJECT...]";
pub const PROJECTS: [&str; 3] = ["north", "makgora", "wisp"];

pub struct Row {
    pub project: String,
    pub concern: Concern,
    pub verdict: Verdict,
}

pub fn code_root() -> PathBuf {
    std::env::var_os("NORTH_CODE_ROOT").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code"), PathBuf::from)
}

fn checkout(code: &Path, project: &str) -> PathBuf {
    code.join(project).join("main")
}

pub fn check(code: &Path, projects: &[String]) -> (Vec<Row>, Vec<String>) {
    let mut wanted: Vec<(String, PathBuf, Concern)> = Vec::new();
    let mut problems = Vec::new();
    for project in projects {
        let root = checkout(code, project);
        let path = root.join(registry::PATH);
        match std::fs::read_to_string(&path).map_err(|why| vec![format!("{}: {why}", path.display())]).and_then(|text| registry::parse(&text).map_err(|found| found.into_iter().map(|why| format!("{project}: {why}")).collect())) {
            Ok(concerns) => wanted.extend(concerns.into_iter().filter(|concern| concern.class != registry::Class::Ready).map(|concern| (project.clone(), root.clone(), concern))),
            Err(found) => problems.extend(found),
        }
    }
    let oracles: Vec<(&Path, &str)> = wanted.iter().map(|(_, root, concern)| (root.as_path(), concern.oracle.as_str())).collect();
    let readings = oracle::measure_all(&oracles);
    let rows = wanted.into_iter().zip(readings).map(|((project, _, concern), reading)| {
        let verdict = judge(&concern, reading);
        Row { project, concern, verdict }
    });
    (rows.collect(), problems)
}

fn number(value: f64, unit: &str) -> String {
    let text = if (value - value.round()).abs() < 1e-9 { format!("{value:.0}") } else { format!("{value:.2}") };
    if unit.is_empty() { text } else { format!("{text} {unit}") }
}

fn measured(row: &Row) -> String {
    match &row.verdict {
        Verdict::Green(value) | Verdict::RedContract(value) | Verdict::RedDebt(value) | Verdict::RedOverCeiling(value) => number(*value, &row.concern.unit),
        Verdict::Unmeasured(why) => why.clone(),
    }
}

pub fn line(row: &Row) -> String {
    let concern = &row.concern;
    let measured = measured(row);
    let class = match &concern.debt {
        Some(debt) => format!("{}, ceiling {}, owner {}, revisit {}", concern.class.word(), number(debt.ceiling, &concern.unit), debt.owner, debt.revisit),
        None => concern.class.word().to_owned(),
    };
    format!("{}/{}: {} {measured} ({} {}); {class}", row.project, concern.name, row.verdict.word(), concern.bound.word(), number(concern.bound.limit(), &concern.unit))
}

pub fn render(rows: &[Row], problems: &[String], seconds: f64) -> String {
    let red = rows.iter().filter(|row| !row.verdict.green()).count();
    let mut out = format!("north check concerns: {} concerns, {} green, {red} red, in {seconds:.1} s\n", rows.len(), rows.len() - red);
    let mut project = "";
    for row in rows {
        if row.project != project {
            project = &row.project;
            let _ = writeln!(out, "\n{project}");
        }
        let _ = writeln!(out, "- {}", line(row).trim_start_matches(&format!("{}/", row.project)));
        let _ = writeln!(out, "  source: {}", row.concern.source);
        let _ = writeln!(out, "  oracle: {}", row.concern.oracle);
    }
    problems.iter().for_each(|problem| {
        let _ = writeln!(out, "\nnot read: {problem}");
    });
    out
}

pub fn failing(rows: &[Row], problems: &[String]) -> bool {
    !problems.is_empty() || rows.iter().any(|row| row.verdict.failing())
}

pub fn run(arguments: &[String], history: &dyn Store) -> u8 {
    let mut code = code_root();
    let mut trend = false;
    let mut projects: Vec<String> = Vec::new();
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--root" => match rest.next() {
                Some(root) => code = PathBuf::from(root),
                None => {
                    eprintln!("{USAGE}");
                    return 2;
                }
            },
            "--trend" => trend = true,
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
    if trend {
        print!("{}", render_trend(&rows, &problems, history));
    } else {
        print!("{}", render(&rows, &problems, began.elapsed().as_secs_f64()));
    }
    u8::from(failing(&rows, &problems))
}

fn render_trend(rows: &[Row], problems: &[String], history: &dyn Store) -> String {
    let now = Timestamp::now().unix_seconds();
    let host = north_base::platform::host::name();
    let mut out = String::new();
    let recorded: Vec<Recorded> = history.read(&host, Timestamp::from_unix_seconds(now - trend::LONGEST)).unwrap_or_else(|why| {
        let _ = writeln!(out, "history not read: {why}");
        Vec::new()
    });
    let _ = writeln!(out, "north check concerns --trend: {} concerns, history of {host}", rows.len());
    let mut project = "";
    for row in rows {
        if row.project != project {
            project = &row.project;
            let _ = writeln!(out, "\n{project}");
        }
        let (state, judged) = tick::state_of(row, &recorded, now);
        let concern = &row.concern;
        let line = match &concern.debt {
            Some(debt) => format!("{} {}, ceiling {}", concern.bound.word(), number(concern.bound.limit(), &concern.unit), number(debt.ceiling, &concern.unit)),
            None => format!("{} {}", concern.bound.word(), number(concern.bound.limit(), &concern.unit)),
        };
        let meets = match (&concern.debt, judged) {
            (Some(_), Some(judged)) => if judged.meets { "; meets its target" } else { "; short of its target" },
            _ => "",
        };
        let _ = writeln!(out, "- {}: {} {} ({line}){meets}", concern.name, state.word(), measured(row));
        if let Some(judged) = judged {
            let windows: Vec<String> = trend::WINDOWS
                .iter()
                .zip(judged.fits)
                .map(|(window, fit)| match fit {
                    Some(fit) => format!("{} median {}, {:+.3}/day", window.name, number(fit.median, &concern.unit), fit.per_day()),
                    None => format!("{} no samples", window.name),
                })
                .collect();
            let _ = writeln!(out, "  {}", windows.join("; "));
        }
    }
    problems.iter().for_each(|problem| {
        let _ = writeln!(out, "\nnot read: {problem}");
    });
    out
}
