mod check;
mod hosts;

pub use check::{Checked, Reading, check};
pub use hosts::{Hosts, last_written};
use std::path::PathBuf;
use std::time::SystemTime;
use toml_edit::{DocumentMut, Item, Table};

pub static REGISTRY: std::sync::LazyLock<&str> = std::sync::LazyLock::new(|| crate::platform::harness::table("harness/loops.toml"));
const USAGE: &str = "usage: north loops check [--role ledger|lander|here]";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Ledger,
    Lander,
    Here,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ledger => "ledger",
            Self::Lander => "lander",
            Self::Here => "here",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        [Self::Ledger, Self::Lander, Self::Here].into_iter().find(|role| role.name() == name)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    pub name: String,
    pub decision: String,
    pub sites: Vec<String>,
    pub constants: Vec<String>,
    pub defaults: Vec<String>,
    pub signal: Option<String>,
    pub updated_by: String,
    pub cadence_hours: u64,
    pub record: Option<String>,
    pub host: Role,
    pub owner: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plumbing {
    pub constants: Vec<String>,
    pub defaults: Vec<String>,
}

fn strings(table: &Table, key: &str) -> Vec<String> {
    table.get(key).and_then(Item::as_array).map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

fn document(text: &str) -> Result<DocumentMut, String> {
    text.parse().map_err(|error| format!("loops registry: {error}"))
}

pub fn plumbing(text: &str) -> Result<Plumbing, String> {
    let document = document(text)?;
    let table = document.get("plumbing").and_then(Item::as_table).ok_or("loops registry has no [plumbing]")?;
    Ok(Plumbing { constants: strings(table, "constants"), defaults: strings(table, "defaults") })
}

pub fn parse(text: &str) -> Result<Vec<Loop>, String> {
    let document = document(text)?;
    let tables = document.get("loop").and_then(Item::as_array_of_tables).ok_or("loops registry has no [[loop]]")?;
    tables.iter().map(|table| {
        let text = |key: &str| table.get(key).and_then(Item::as_str).map(str::to_owned);
        let name = text("name").ok_or("a loop has no name")?;
        let needs = |key: &str| text(key).ok_or_else(|| format!("loop {name} has no {key}"));
        let sites = strings(table, "sites");
        if sites.is_empty() {
            return Err(format!("loop {name} names no sites"));
        }
        let cadence_hours = table.get("cadence_hours").and_then(Item::as_integer).filter(|hours| *hours > 0).ok_or_else(|| format!("loop {name} has no positive cadence_hours"))?;
        let (signal, record, owner) = (text("signal"), text("record"), text("owner"));
        if signal.is_some() != record.is_some() {
            return Err(format!("loop {name} names a signal and a record together or neither"));
        }
        if signal.is_none() && owner.is_none() {
            return Err(format!("loop {name} has no signal and no owner to close it"));
        }
        let host = match text("host") {
            None => Role::Here,
            Some(role) => Role::parse(&role).ok_or_else(|| format!("loop {name} has host {role}, not ledger, lander or here"))?,
        };
        Ok(Loop { decision: needs("decision")?, updated_by: needs("updated_by")?, constants: strings(table, "constants"), defaults: strings(table, "defaults"), name, sites, signal, cadence_hours: cadence_hours.unsigned_abs(), record, host, owner })
    }).collect()
}

pub fn state_root() -> PathBuf {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    var("XDG_STATE_HOME").unwrap_or_else(|| var("HOME").unwrap_or_default().join(".local/state"))
}

pub fn here(role: Option<Role>) -> Result<Checked, String> {
    let loops = parse(&REGISTRY)?;
    let state = state_root();
    let read = |record: &str| last_written(&state, record).map_or(Reading::Never, Reading::At);
    let held: Box<dyn Fn(Role) -> bool> = match role {
        Some(role) => Box::new(move |found| found == role),
        None => {
            let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
            let hosts = Hosts::read(&home, crate::platform::harness::harness_root());
            Box::new(move |found| hosts.holds(found))
        }
    };
    Ok(check(&loops, &*held, &read, SystemTime::now()))
}

pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let role = match words[..] {
        ["check"] => None,
        ["check", "--role", name] => match Role::parse(name) {
            Some(role) => Some(role),
            None => {
                eprintln!("north loops: role {name} is not ledger, lander or here");
                return 2;
            }
        },
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    match here(role) {
        Ok(checked) => {
            checked.lines.iter().for_each(|line| println!("{line}"));
            u8::from(!checked.failing.is_empty())
        }
        Err(error) => {
            eprintln!("north loops: {error}");
            1
        }
    }
}

