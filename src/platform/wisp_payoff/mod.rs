use crate::error::{NorthError, NorthResult};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use time::{Date, Duration, Month, OffsetDateTime};
mod contract;
mod evidence;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Serialize)]
pub struct Run {
    pub path: PathBuf,
    pub kind: String,
    pub script: Option<String>,
    pub at: i64,
    pub client_minutes: f64,
    pub estimated: bool,
    pub agreement: Option<bool>,
    pub invalid: bool,
}

#[derive(Default, Debug, Serialize)]
pub struct Counts {
    pub runs: usize,
    pub client_minutes: f64,
    pub estimated_minutes: f64,
    pub compared: usize,
    pub agrees: usize,
    pub disagrees: usize,
    pub invalid: usize,
}
impl Counts {
    fn add(&mut self, run: &Run) {
        self.runs += 1;
        self.client_minutes += run.client_minutes;
        if run.estimated {
            self.estimated_minutes += run.client_minutes;
        }
        self.invalid += usize::from(run.invalid);
        if let Some(agrees) = run.agreement {
            self.compared += 1;
            self.agrees += usize::from(agrees);
            self.disagrees += usize::from(!agrees);
        }
    }
}
#[derive(Debug, Serialize)]
pub struct KindWeek {
    pub week: String,
    pub kind: String,
    pub counts: Counts,
}
#[derive(Debug, Serialize)]
pub struct Week {
    pub week: String,
    pub native_client_minutes: f64,
    pub wisp_commits: Option<usize>,
    pub native_minutes_avoided: Option<f64>,
    pub projected_sample_saving_minutes: f64,
    pub closed_public_boxes: Option<usize>,
    pub minutes_per_closed_box: Option<f64>,
    pub wisp_only_share: Option<f64>,
}
#[derive(Debug, Serialize)]
pub struct Fix {
    pub kind: String,
    pub evidence: Vec<PathBuf>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub families: Vec<contract::Family>,
    pub sampler: contract::Sampler,
    pub kinds: Vec<KindWeek>,
    pub weeks: Vec<Week>,
    pub retire: Vec<String>,
    pub fixes: Vec<Fix>,
    pub files: Vec<PathBuf>,
    pub runs: Vec<Run>,
    pub warnings: Vec<String>,
}

pub fn run(arguments: &[String]) -> NorthResult<()> {
    let mut since = None;
    let mut json = false;
    let mut args = arguments.iter();
    if args.next().map(String::as_str) != Some("payoff") {
        return Err(usage());
    }
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--since" => since = Some(parse_date(args.next().ok_or_else(usage)?)?),
            _ => return Err(usage()),
        }
    }
    let report = collect(since)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", report.render());
    }
    Ok(())
}
fn usage() -> NorthError {
    NorthError::Usage("Usage: north wisp payoff [--since YYYY-MM-DD] [--json]".into())
}
fn parse_date(value: &str) -> NorthResult<Date> {
    let parts: Vec<_> = value.split('-').collect();
    let parsed = (|| {
        if parts.len() != 3 {
            return None;
        }
        Date::from_calendar_date(
            parts[0].parse().ok()?,
            Month::try_from(parts[1].parse::<u8>().ok()?).ok()?,
            parts[2].parse().ok()?,
        )
        .ok()
    })();
    parsed.ok_or_else(usage)
}
pub fn collect(since: Option<Date>) -> NorthResult<Report> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| NorthError::Configuration("HOME missing".into()))?;
    let roots = [
        home.join(".local/state/smashcraft/evidence"),
        home.join(".local/state/wisp"),
        home.join("code/wisp/main/evidence"),
        home.join("code/wisp/main/builds"),
    ];
    let mut warnings = Vec::new();
    let mut files = BTreeSet::new();
    for root in roots {
        evidence::discover(&root, &mut files, &mut warnings);
    }
    let mut runs = Vec::new();
    for path in &files {
        match evidence::read(path) {
            Ok(found) => runs.extend(found),
            Err(error) => warnings.push(format!("{}: {error}", path.display())),
        }
    }
    let mut report = summarize(runs, since, OffsetDateTime::now_utc().unix_timestamp());
    if !warnings.is_empty() {
        report.retire.clear();
        for family in &mut report.families {
            family.verdict = "native";
            family.native_sample_rate = 1.0;
        }
        for row in &mut report.weeks {
            row.projected_sample_saving_minutes = 0.0;
        }
        warnings.push("Incomplete evidence scan: retirement recommendations suppressed".into());
    }
    report.files = files.into_iter().collect();
    report.warnings.extend(warnings);
    commits(&home.join("code/wisp/main"), &mut report);
    Ok(report)
}
fn week(at: i64) -> String {
    let date = OffsetDateTime::from_unix_timestamp(at)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .date();
    (date - Duration::days(i64::from(date.weekday().number_days_from_monday()))).to_string()
}
fn summarize(runs: Vec<Run>, since: Option<Date>, now: i64) -> Report {
    let families = contract::families(&runs, now);
    let mut fixes: BTreeMap<String, BTreeSet<PathBuf>> = BTreeMap::new();
    let retire: Vec<String> = families
        .iter()
        .filter(|family| family.verdict == "wisp-only")
        .map(|family| family.id.clone())
        .collect();
    let start = since
        .map(|date| date.midnight().assume_utc().unix_timestamp())
        .unwrap_or(i64::MIN);
    let runs: Vec<_> = runs
        .into_iter()
        .filter(|run| run.at >= start && run.at <= now)
        .collect();
    let mut kinds: BTreeMap<(String, String), Counts> = BTreeMap::new();
    for run in &runs {
        kinds
            .entry((week(run.at), run.kind.clone()))
            .or_default()
            .add(run);
        if run.agreement == Some(false) {
            fixes
                .entry(run.kind.clone())
                .or_default()
                .insert(run.path.clone());
        }
    }
    let mut weeks = weekly(&runs, &kinds, now);
    for row in &mut weeks {
        row.projected_sample_saving_minutes = kinds
            .iter()
            .filter(|((w, kind), _)| *w == row.week && retire.contains(kind))
            .map(|(_, c)| c.client_minutes * 0.9)
            .sum();
    }
    Report {
        schema_version: 1,
        families,
        sampler: contract::Sampler::default(), kinds: kinds.into_iter().map(|((week, kind), counts)| KindWeek {week, kind, counts}).collect(), weeks, retire, fixes: fixes.into_iter().map(|(kind, paths)| Fix {kind, evidence: paths.into_iter().collect()}).collect(), files: Vec::new(), runs, warnings: vec![
        "Pad minutes = 2 clients × (game + reset + reload + run)/60; includes failed attempts, excludes Wisp/compare/wait. Accept elapsed time is allocated equally across checks assuming 2 clients: an estimate, not per-check timing. Batch timestamps fall back to file modification time.".into(),
        "Release-box closure history and Wisp-only closure provenance are absent from these files: release denominators/shares are unavailable. No recorded retirement or skipped-run ledger: avoided minutes are unavailable; a recommendation is not an observed saving.".into()] }
}
fn weekly(runs: &[Run], kinds: &BTreeMap<(String, String), Counts>, now: i64) -> Vec<Week> {
    let mut rows = Vec::new();
    if let Some(first) = runs.iter().map(|run| run.at).min() {
        let date = OffsetDateTime::from_unix_timestamp(first).unwrap().date();
        let mut date = date - Duration::days(i64::from(date.weekday().number_days_from_monday()));
        while date.midnight().assume_utc().unix_timestamp() <= now {
            let key = date.to_string();
            let minutes = kinds
                .iter()
                .filter(|((w, _), _)| *w == key)
                .map(|(_, c)| c.client_minutes)
                .sum();
            rows.push(Week {
                week: key,
                native_client_minutes: minutes,
                wisp_commits: None,
                native_minutes_avoided: None,
                projected_sample_saving_minutes: 0.0,
                closed_public_boxes: None,
                minutes_per_closed_box: None,
                wisp_only_share: None,
            });
            date += Duration::days(7);
        }
    }
    rows
}
fn commits(repo: &Path, report: &mut Report) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["log", "main", "--format=%ct"])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let mut totals: BTreeMap<String, usize> = BTreeMap::new();
            for at in String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|s| s.parse::<i64>().ok())
            {
                *totals.entry(week(at)).or_default() += 1;
            }
            for row in &mut report.weeks {
                row.wisp_commits = Some(*totals.get(&row.week).unwrap_or(&0));
            }
        }
        _ => report.warnings.push(format!(
            "Wisp main commit history unavailable: {}",
            repo.display()
        )),
    }
}
impl Report {
    pub fn summary(&self) -> String {
        let agrees: usize = self.kinds.iter().map(|k| k.counts.agrees).sum();
        let compared: usize = self.kinds.iter().map(|k| k.counts.compared).sum();
        let minutes: f64 = self.kinds.iter().map(|k| k.counts.client_minutes).sum();
        format!(
            "Wisp payoff: {agrees}/{compared} agree · {minutes:.1} native client-min* · {} retire · {} fix · savings unknown",
            self.retire.len(),
            self.fixes.len()
        )
    }
    pub fn render(&self) -> String {
        let mut out = format!(
            "{}\nweek       kind                         runs client-min agree/compared disagree invalid\n",
            self.summary()
        );
        for row in &self.kinds {
            let c = &row.counts;
            out.push_str(&format!(
                "{} {:<28} {:>4} {:>10.2} {:>4}/{:<8} {:>8} {:>7}\n",
                row.week,
                row.kind,
                c.runs,
                c.client_minutes,
                c.agrees,
                c.compared,
                c.disagrees,
                c.invalid
            ));
        }
        out.push_str("\nWeekly trend (UTC Monday)\nweek       native-client-min Wisp-commits avoided-min closed-boxes min/box Wisp-only-share projected-sample-saving-min\n");
        for row in &self.weeks {
            out.push_str(&format!(
                "{} {:>17.2} {:>12}     unknown      unknown unknown unknown {:>12.2}\n",
                row.week,
                row.native_client_minutes,
                row.wisp_commits.map_or("unknown".into(), |n| n.to_string()),
                row.projected_sample_saving_minutes
            ));
        }
        out.push_str("\nRetire (last 14 days: >=10 agrees, zero disagreements): Wisp only; native sample 1 in 10\n");
        if self.retire.is_empty() {
            out.push_str("  none\n");
        }
        for kind in &self.retire {
            out.push_str(&format!("  {kind}\n"));
        }
        out.push_str(
            "Fix (completed disagreements; investigate attribution before changing Wisp)\n",
        );
        if self.fixes.is_empty() {
            out.push_str("  none\n");
        }
        for fix in &self.fixes {
            out.push_str(&format!("  {}\n", fix.kind));
            for path in &fix.evidence {
                out.push_str(&format!("    {}\n", path.display()));
            }
        }
        for warning in &self.warnings {
            out.push_str(&format!("Note: {warning}\n"));
        }
        out
    }
}
