use std::collections::BTreeMap;

use super::fail;
use super::json::{number, show};
use super::policy::{CLASS_NAMES, native_memory_request, resource_class};
use super::request::Request;

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn safe_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER
}

pub fn positive_integer(text: &str, option: &str, maximum: Option<u64>) -> u64 {
    let value = number(text);
    let maximum = maximum.map_or(MAX_SAFE_INTEGER, |maximum| maximum as f64);
    if !safe_integer(value) || value < 1.0 || value > maximum {
        fail(&format!("{option} must be an integer from 1 to {}", show(maximum)), 2);
    }
    value as u64
}

pub fn nonnegative_integer(text: &str, option: &str) -> u64 {
    let value = number(text);
    if !safe_integer(value) || value < 0.0 {
        fail(&format!("{option} must be a nonnegative integer"), 2);
    }
    value as u64
}

/// `--option value` pairs up to an optional `--` that starts a command.
pub struct Options {
    values: BTreeMap<String, String>,
    pub separator: usize,
}

impl Options {
    pub fn parse(argv: &[String], start: usize, allowed: &[&str], flags: &[&str]) -> Self {
        let mut values = BTreeMap::new();
        let mut index = start;
        while index < argv.len() {
            let option = &argv[index];
            if option == "--" {
                return Self { values, separator: index };
            }
            let flag = flags.contains(&option.as_str());
            let value = if flag { Some("true".to_owned()) } else { argv.get(index + 1).cloned() };
            let Some(value) = value.filter(|_| option.starts_with("--")) else {
                fail(&format!("invalid argument at position {}", index + 1), 2);
            };
            if !allowed.contains(&option.as_str()) {
                fail(&format!("unknown option: {option}"), 2);
            }
            if values.insert(option.clone(), value).is_some() {
                fail(&format!("duplicate option: {option}"), 2);
            }
            index += if flag { 1 } else { 2 };
        }
        Self { values, separator: argv.len() }
    }

    pub fn has(&self, option: &str) -> bool {
        self.values.contains_key(option)
    }

    pub fn get(&self, option: &str) -> Option<&str> {
        self.values.get(option).map(String::as_str)
    }

    pub fn required(&self, option: &str) -> &str {
        self.get(option).filter(|value| !value.is_empty()).unwrap_or_else(|| fail(&format!("missing {option}"), 2))
    }

    pub fn owner(&self, option: &str) -> String {
        let owner = self.required(option);
        let stable = (1..=160).contains(&owner.chars().count()) && owner.chars().all(|character| character.is_ascii_alphanumeric() || "_.:@/-".contains(character));
        if !stable {
            fail(&format!("{option} must be a stable actor label without whitespace"), 2);
        }
        owner.to_owned()
    }

    fn count(&self, option: &str, name: &str) -> Option<u32> {
        let text = self.get(option)?;
        if name != "native" {
            fail(&format!("{option} declares a native request's {}", option.get(6..).unwrap_or_default()), 2);
        }
        Some(u32::try_from(nonnegative_integer(text, option)).unwrap_or(u32::MAX))
    }

    fn resources(&self, name: &str, cores: f64) -> super::policy::Resources {
        let mut resources = resource_class(name, cores).unwrap_or_else(|| fail(&format!("policy rejected resource class: {name}"), 2));
        if self.has("--memory-gib") {
            let memory_gib = number(self.required("--memory-gib"));
            if !memory_gib.is_finite() {
                fail("--memory-gib must be finite", 2);
            }
            resources = native_memory_request(name, cores, memory_gib).unwrap_or_else(|message| fail(message, 2));
            if !safe_integer(resources.memory_mib) {
                fail("--memory-gib must represent a whole, safe number of MiB", 2);
            }
        }
        if !resources.cpus.is_finite() || !resources.memory_mib.is_finite() {
            fail(&format!("policy rejected resource class: {name}"), 2);
        }
        resources
    }

    /// The declared class and its resources, with a native request's client and desktop starts.
    pub fn class(&self, cores: f64) -> Request {
        let name = self.required("--class");
        if !CLASS_NAMES.contains(&name) {
            fail("--class must be agent, native, moderate, heavy, exclusive, gpu, critical, tom, or runtime", 2);
        }
        let resources = self.resources(name, cores);
        let new_clients = self.count("--new-clients", name).unwrap_or(u32::from(name == "native"));
        let new_desktops = self.count("--new-desktops", name).unwrap_or(0);
        let pair = self.count("--pair", name);
        let wait_seconds = self.get("--wait-seconds").map(|text| {
            if new_clients + new_desktops == 0 {
                fail("--wait-seconds queues a native client or desktop start (--new-clients or --new-desktops above 0)", 2);
            }
            positive_integer(text, "--wait-seconds", None)
        });
        Request { name: name.to_owned(), resources: Some(resources), new_clients, new_desktops, pair, wait_seconds, ..Request::default() }
    }
}
