use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const RESIDENTS: [Resident; 2] = [Resident::SessionHost, Resident::SessionLink];
const LANDER: &str = "land serve";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Resident {
    SessionHost,
    SessionLink,
}

impl Resident {
    fn of(argv: &[&str]) -> Option<Self> {
        match argv {
            ["session-host", ..] => Some(Self::SessionHost),
            ["session-link", ..] => Some(Self::SessionLink),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::SessionHost => "session-host",
            Self::SessionLink => "session-link",
        }
    }
}

fn promoted(program: &str) -> bool {
    program.split('/').any(|part| part == "runtimes")
}

fn lander_unit(unit: &str) -> bool {
    unit.starts_with("north-land") && unit.ends_with(".service")
}

fn lander(argv: &[&str]) -> Option<String> {
    match argv {
        ["land", "serve", "--all"] => Some("all repositories".to_owned()),
        ["land", "serve", "--repo", repo] => Some((*repo).to_owned()),
        _ => None,
    }
}

enum Role<'a> {
    Resident(Resident),
    Lander { repo: String, unit: &'a str },
    Transient,
}

struct Row<'a> {
    pid: &'a str,
    parent: &'a str,
    age: u64,
    unit: &'a str,
    args: Vec<&'a str>,
    role: Role<'a>,
}

impl<'a> Row<'a> {
    fn parse(line: &'a str) -> Option<Self> {
        let mut fields = line.splitn(5, '\t');
        let (Some(pid), Some(parent), Some(age), Some(unit), Some(command)) = (fields.next(), fields.next(), fields.next(), fields.next(), fields.next()) else { return None };
        let argv: Vec<&str> = command.split('\t').collect();
        let (program, args) = (argv[0], argv[1..].to_vec());
        let (unit, service) = if lander_unit(unit) { ("", unit) } else { (unit, "") };
        let role = Self::role(program, &args, service);
        Some(Self { pid, parent, age: age.parse().unwrap_or(0), unit, args, role })
    }

    fn role(program: &'a str, args: &[&'a str], service: &'a str) -> Role<'a> {
        if let Some(resident) = Resident::of(args).filter(|_| promoted(program)) {
            return Role::Resident(resident);
        }
        match lander(args).filter(|_| promoted(program) && !service.is_empty()) {
            Some(repo) => Role::Lander { repo, unit: service },
            None => Role::Transient,
        }
    }

    fn session_name(&self) -> Option<&'a str> {
        let at = self.args.iter().position(|word| *word == "--name")?;
        self.args.get(at + 1).copied()
    }
}

#[derive(Default)]
pub(super) struct NorthProcesses {
    units: BTreeMap<String, u64>,
    outlasting: u64,
    live_units: BTreeSet<String>,
    residents: BTreeMap<Resident, u64>,
    sessions: BTreeSet<String>,
    landers: BTreeMap<String, BTreeSet<String>>,
}

impl NorthProcesses {
    pub(super) fn read(section: &str, held: &str, window: u64) -> Self {
        let rows: Vec<Row> = section.lines().filter(|line| !line.is_empty()).filter_map(Row::parse).collect();
        let transient_pids: BTreeSet<&str> = rows.iter().filter(|row| matches!(row.role, Role::Transient)).map(|row| row.pid).collect();
        let mut processes = Self { live_units: held.lines().map(str::trim).filter(|unit| !unit.is_empty()).map(str::to_owned).collect(), ..Self::default() };
        for row in &rows {
            processes.count(row, &transient_pids, window);
        }
        processes
    }

    fn count(&mut self, row: &Row, transient_pids: &BTreeSet<&str>, window: u64) {
        match &row.role {
            Role::Resident(resident) => self.resident(row, *resident),
            Role::Lander { repo, unit } => {
                self.landers.entry(repo.clone()).or_default().insert((*unit).to_owned());
            }
            Role::Transient if !transient_pids.contains(row.parent) => {
                *self.units.entry(row.unit.to_owned()).or_default() += 1;
                self.outlasting += u64::from(row.unit.is_empty() && row.age >= window);
            }
            Role::Transient => {}
        }
    }

    fn resident(&mut self, row: &Row, resident: Resident) {
        *self.residents.entry(resident).or_default() += 1;
        if resident == Resident::SessionHost {
            self.sessions.extend(row.session_name().map(str::to_owned));
        }
    }

    fn resident_count(&self, resident: Resident) -> u64 {
        self.residents.get(&resident).copied().unwrap_or_default()
    }

    pub(super) fn outlasting(&self) -> u64 {
        self.outlasting
    }

    fn owned(&self) -> u64 {
        self.units.iter().filter(|(unit, _)| self.live_units.contains(*unit)).map(|(_, count)| count).sum()
    }

    fn ended(&self) -> impl Iterator<Item = (&String, &u64)> {
        self.units.iter().filter(|(unit, _)| !unit.is_empty() && !self.live_units.contains(*unit))
    }

    fn transient(&self) -> u64 {
        self.units.values().sum()
    }

    pub(super) fn growth(&self) -> String {
        format!("transient growth: {} for {} live sessions", self.owned(), self.live_units.len())
    }

    pub(super) fn leak(&self, window: Option<&str>) -> String {
        let leaks: Vec<String> = self.ended_leaks().into_iter().chain(window.map(str::to_owned)).collect();
        match leaks.is_empty() {
            true => "transient leak: none".to_owned(),
            false => format!("transient leak: {}", leaks.join("; ")),
        }
    }

    fn ended_leaks(&self) -> Vec<String> {
        self.ended().map(|(unit, count)| format!("{count} transient north processes outlived their ended session {unit}")).collect()
    }

    pub(super) fn problems(&self) -> Vec<String> {
        let live = self.sessions.len() as u64;
        let mut problems = Vec::new();
        for resident in RESIDENTS {
            let count = self.resident_count(resident);
            if count > live {
                problems.push(format!("{count} {} processes for {live} live sessions", resident.name()));
            }
        }
        for (repo, units) in self.landers.iter().filter(|(_, units)| units.len() > 1) {
            problems.push(format!("{} {LANDER} units for {repo}: {}", units.len(), units.iter().cloned().collect::<Vec<_>>().join(", ")));
        }
        problems.extend(self.ended_leaks());
        problems
    }

    pub(super) fn record(&self) -> Value {
        let mut residents: serde_json::Map<String, Value> = RESIDENTS.iter().map(|resident| (resident.name().to_owned(), json!(self.resident_count(*resident)))).collect();
        residents.insert(LANDER.to_owned(), json!(self.landers.values().map(BTreeSet::len).sum::<usize>()));
        json!({
            "transient": self.transient(),
            "outlasting": self.outlasting,
            "liveSessions": self.sessions.len(),
            "residents": residents,
        })
    }
}
