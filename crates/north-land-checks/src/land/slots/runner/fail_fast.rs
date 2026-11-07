use crate::landing::check_timings::outcomes::{Outcome, Record};

pub const CHEAP_CPU_SECONDS: f64 = 120.0;
const WINDOW: usize = 20;
const FINEST: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Estimate {
    fail_rate: f64,
    passed_cpu: f64,
    failed_cpu: f64,
}

impl Estimate {
    fn of(rows: &[&Record]) -> Option<Self> {
        let completed: Vec<&Record> = rows.iter().rev().filter(|row| row.outcome != Outcome::Cancelled).take(WINDOW).copied().collect();
        if completed.is_empty() {
            return None;
        }
        let cost = |outcome: Outcome| median(completed.iter().filter(|row| row.outcome == outcome).map(|row| row.cost()).collect());
        let failed = completed.iter().filter(|row| row.outcome == Outcome::Failed).count();
        let (passed_cpu, failed_cpu) = match (cost(Outcome::Passed), cost(Outcome::Failed)) {
            (Some(passed), Some(failed)) => (passed, failed),
            (Some(only), None) | (None, Some(only)) => (only, only),
            (None, None) => return None,
        };
        #[allow(clippy::cast_precision_loss)]
        let fail_rate = (failed as f64 + 1.0) / (completed.len() as f64 + 2.0);
        Some(Estimate { fail_rate, passed_cpu, failed_cpu })
    }

    fn predicted_cpu(self) -> f64 {
        self.passed_cpu.max(self.failed_cpu)
    }

    fn fails_per_cpu_second(self) -> f64 {
        let expected = self.fail_rate * self.failed_cpu + (1.0 - self.fail_rate) * self.passed_cpu;
        self.fail_rate / expected.max(1.0)
    }
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) { f64::midpoint(values[middle - 1], values[middle]) } else { values[middle] })
}

fn estimate(history: &[Record], command: &str, paths: &str) -> Option<Estimate> {
    let rows: Vec<&Record> = history.iter().filter(|row| row.command == command).collect();
    let finest: Vec<&Record> = rows.iter().filter(|row| row.paths == paths).copied().collect();
    let completed = finest.iter().filter(|row| row.outcome != Outcome::Cancelled).count();
    Estimate::of(if completed >= FINEST { &finest } else { &rows })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub cheap: Vec<String>,
    pub expensive: Vec<String>,
}

impl Plan {
    pub fn of(commands: &[String], paths: &str, history: &[Record]) -> Self {
        let mut plan = Plan::default();
        let mut ranked = Vec::new();
        for command in commands {
            match estimate(history, command, paths) {
                Some(estimate) if estimate.predicted_cpu() >= CHEAP_CPU_SECONDS => ranked.push((estimate.fails_per_cpu_second(), command.clone())),
                _ => plan.cheap.push(command.clone()),
            }
        }
        ranked.sort_by(|left, right| right.0.total_cmp(&left.0));
        plan.expensive = ranked.into_iter().map(|(_, command)| command).collect();
        plan
    }

    pub fn is_expensive(&self, command: &str) -> bool {
        self.expensive.iter().any(|expensive| expensive == command)
    }

    pub fn ordered(&self) -> impl Iterator<Item = &String> {
        self.cheap.iter().chain(&self.expensive)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Replay {
    pub failing: usize,
    pub passing: usize,
    pub today_cpu: f64,
    pub ordered_cpu: f64,
    pub today_wall: f64,
    pub ordered_wall: f64,
}

struct Landing<'a> {
    checks: Vec<&'a Record>,
}

impl Landing<'_> {
    fn failed(&self) -> bool {
        self.checks.iter().any(|check| check.outcome == Outcome::Failed)
    }

    fn cpu_until_first_failure<'b>(order: impl Iterator<Item = &'b Record>) -> f64 {
        let mut spent = 0.0;
        for check in order {
            spent += check.cost();
            if check.outcome == Outcome::Failed {
                break;
            }
        }
        spent
    }

    fn today_cpu(&self) -> f64 {
        Self::cpu_until_first_failure(self.checks.iter().copied())
    }

    fn plan(&self, history: &[Record]) -> Plan {
        let commands: Vec<String> = self.checks.iter().map(|check| check.command.clone()).collect();
        Plan::of(&commands, &self.checks[0].paths, history)
    }

    fn ordered_cpu(&self, history: &[Record]) -> f64 {
        let plan = self.plan(history);
        let check = |command: &String| self.checks.iter().copied().find(|check| &check.command == command);
        let cheap: Vec<&Record> = plan.cheap.iter().filter_map(check).collect();
        let cheap_cpu: f64 = cheap.iter().map(|check| check.cost()).sum();
        if cheap.iter().any(|check| check.outcome == Outcome::Failed) {
            return cheap_cpu;
        }
        cheap_cpu + Self::cpu_until_first_failure(plan.expensive.iter().filter_map(check))
    }

    fn today_wall(&self) -> f64 {
        self.checks.iter().map(|check| check.wall).sum()
    }

    fn ordered_wall(&self, history: &[Record]) -> f64 {
        self.plan(history).ordered().filter_map(|command| self.checks.iter().find(|check| &check.command == command)).map(|check| check.wall).sum()
    }
}

pub fn replay(records: &[Record]) -> Replay {
    let mut landings: Vec<(String, Vec<&Record>)> = Vec::new();
    for record in records {
        match landings.iter_mut().find(|(landing, _)| landing == &record.landing) {
            Some((_, checks)) => checks.push(record),
            None => landings.push((record.landing.clone(), vec![record])),
        }
    }
    let mut landings: Vec<Landing> = landings
        .into_iter()
        .filter(|(_, checks)| checks.iter().all(|check| check.outcome != Outcome::Cancelled))
        .map(|(_, mut checks)| {
            checks.sort_by_key(|check| check.started);
            Landing { checks }
        })
        .collect();
    landings.sort_by_key(|landing| landing.checks[0].started);
    let (mut today_cpu, mut ordered_cpu, mut today_wall, mut ordered_wall) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for landing in &landings {
        let first = landing.checks[0].started;
        let history: Vec<Record> = records.iter().filter(|record| record.started < first && record.landing != landing.checks[0].landing).cloned().collect();
        if landing.failed() {
            today_cpu.push(landing.today_cpu());
            ordered_cpu.push(landing.ordered_cpu(&history));
        } else {
            today_wall.push(landing.today_wall());
            ordered_wall.push(landing.ordered_wall(&history));
        }
    }
    let middle = |values: &[f64]| median(values.to_vec()).unwrap_or_default();
    Replay {
        failing: today_cpu.len(),
        passing: today_wall.len(),
        today_cpu: middle(&today_cpu),
        ordered_cpu: middle(&ordered_cpu),
        today_wall: middle(&today_wall),
        ordered_wall: middle(&ordered_wall),
    }
}

impl std::fmt::Display for Replay {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        let wall_change = if self.today_wall > 0.0 { (self.ordered_wall / self.today_wall - 1.0) * 100.0 } else { 0.0 };
        writeln!(formatter, "landings replayed: {} failing, {} passing", self.failing, self.passing)?;
        writeln!(formatter, "median CPU-s before the failure verdict: {:.1} with today's order and cancel-on-first-failure, {:.1} with the fail-fast order", self.today_cpu, self.ordered_cpu)?;
        write!(formatter, "median passing landing wall: {:.1} s today, {:.1} s with the fail-fast order ({wall_change:+.1}%)", self.today_wall, self.ordered_wall)
    }
}
