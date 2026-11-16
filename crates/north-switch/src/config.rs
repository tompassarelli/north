//! Which hosts `north fleet off` reaches and which of their user units belong to the fleet, read from
//! `$XDG_CONFIG_HOME/north/fleet-units` (one `host NAME`, `unit GLOB` or `keep GLOB` per line), which the host
//! configuration writes; a key the file leaves out keeps its default.

use std::fs;
use std::path::{Path, PathBuf};

use crate::host::valid_name;

const DEFAULT_UNITS: [&str; 2] = ["north-*", "fleet-*"];
const DEFAULT_KEEP: [&str; 1] = ["north-gh-broker.*"];

#[derive(Debug, PartialEq, Eq)]
pub struct Config {
    pub hosts: Vec<String>,
    pub units: Vec<String>,
    pub keep: Vec<String>,
}

impl Config {
    pub fn load(config_home: &Path) -> Result<Self, String> {
        let path = file(config_home);
        match fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map_err(|error| format!("{}: {error}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::parse(""),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    fn parse(text: &str) -> Result<Self, String> {
        let (mut hosts, mut units, mut keep) = (Vec::new(), Vec::new(), Vec::new());
        for (number, line) in text.lines().enumerate().map(|(index, line)| (index + 1, line.trim())) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            match line.split_once(char::is_whitespace).map(|(key, value)| (key, value.trim())) {
                Some(("host", value)) if valid_name(value) => hosts.push(value.to_owned()),
                Some(("unit", value)) => units.push(value.to_owned()),
                Some(("keep", value)) => keep.push(value.to_owned()),
                _ => return Err(format!("line {number} is not `host NAME` (letters, digits and ._-@: , not starting with -), `unit GLOB` or `keep GLOB`: {line}")),
            }
        }
        let or_default = |list: Vec<String>, default: &[&str]| if list.is_empty() { default.iter().map(|glob| (*glob).to_owned()).collect() } else { list };
        Ok(Self { hosts, units: or_default(units, &DEFAULT_UNITS), keep: or_default(keep, &DEFAULT_KEEP) })
    }

    pub fn fleet_unit(&self, unit: &str) -> bool {
        self.units.iter().any(|glob| matches(glob, unit)) && !self.keep.iter().any(|glob| matches(glob, unit))
    }
}

fn file(config_home: &Path) -> PathBuf {
    config_home.join("north/fleet-units")
}

fn matches(glob: &str, text: &str) -> bool {
    let mut parts = glob.split('*');
    let first = parts.next().unwrap_or("");
    let Some(mut rest) = text.strip_prefix(first) else { return false };
    let middle: Vec<&str> = parts.collect();
    let Some((last, inner)) = middle.split_last() else { return rest.is_empty() };
    for part in inner {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

