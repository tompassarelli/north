use std::path::Path;
use std::process::{Command, Stdio};

use toml_edit::{DocumentMut, Item};

use super::report::Report;
use super::Context;

struct Machines<'a> {
    here: String,
    home: &'a Path,
    ssh: Vec<(String, String)>,
}

impl Machines<'_> {
    fn local(&self, host: &str) -> Option<std::path::PathBuf> {
        (host == self.here).then(|| self.home.to_path_buf())
    }
}

impl Machines<'_> {
    fn has_directory(&self, host: &str, path: &str) -> Option<bool> {
        if let Some(home) = self.local(host) {
            return Some(expand(&home, path).is_dir());
        }
        let destination = self.ssh.iter().find(|(name, _)| name == host).map(|(_, destination)| destination)?;
        let rest = path.strip_prefix("~/").unwrap_or(path);
        let remote = if path.starts_with("~/") { format!("test -d \"$HOME/{rest}\"") } else { format!("test -d '{path}'") };
        let status = Command::new("ssh")
            .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, &remote])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()?;
        match status.code() {
            Some(0) => Some(true),
            Some(1) => Some(false),
            _ => None,
        }
    }

    fn tier(&self, host: &str, path: &str) -> Option<String> {
        let directory = expand(&self.local(host)?, path);
        crate::usage::profiles::rate_limit_tier(&directory.join(".claude.json"), &directory.join(".credentials.json"))
    }
}

fn expand(home: &Path, path: &str) -> std::path::PathBuf {
    path.strip_prefix("~/").map_or_else(|| path.into(), |rest| home.join(rest))
}

pub(super) fn check(context: &Context, report: &mut Report) {
    let before = report.fail;
    let (orchestration, costs) = (super::read(&context.operator.join("orchestration.toml")), super::read(&context.operator.join("costs.toml")));
    let machines = Machines { here: crate::platform::host::name(), home: &context.roots.home, ssh: ssh_hosts(&orchestration) };
    let found = problems(&orchestration, &costs, &machines);
    for problem in &found {
        report.bad(problem);
    }
    if found.is_empty() {
        report.ok("every account has a costs.toml service, a plans entry and a profile directory on each declared host");
    }
    report.group("accounts", "orchestration.toml accounts · costs.toml services · plans · profile directories", before);
}

fn ssh_hosts(orchestration: &str) -> Vec<(String, String)> {
    let Ok(document) = orchestration.parse::<DocumentMut>() else { return Vec::new() };
    let Some(hosts) = document.get("hosts").and_then(Item::as_table_like) else { return Vec::new() };
    hosts.iter().filter_map(|(name, host)| Some((name.to_owned(), host.get("ssh")?.as_str()?.to_owned()))).collect()
}

fn problems(orchestration: &str, costs: &str, profiles: &Machines<'_>) -> Vec<String> {
    let (Ok(orchestration), Ok(costs)) = (orchestration.parse::<DocumentMut>(), costs.parse::<DocumentMut>()) else {
        return vec!["orchestration.toml or costs.toml does not parse".to_owned()];
    };
    let services: Vec<&str> = costs.get("service").and_then(Item::as_array_of_tables).into_iter().flatten().filter_map(|service| service.get("id")?.as_str()).collect();
    let mut found = Vec::new();
    let Some(providers) = orchestration.get("accounts").and_then(Item::as_table_like) else { return found };
    for (provider, config) in providers.iter() {
        let plans = config.get("plans").and_then(Item::as_table_like);
        let Some(accounts) = config.get("profiles").and_then(Item::as_table_like) else { continue };
        for (account, hosts) in accounts.iter() {
            let service = format!("{provider}-{account}");
            if !services.iter().any(|id| *id == service || *id == provider) {
                found.push(format!("accounts.{provider}.{account} has no costs.toml service {service}"));
            }
            let plan = plans.and_then(|plans| plans.get(account)).and_then(Item::as_str);
            if plans.is_some() && plan.is_none() {
                found.push(format!("accounts.{provider}.{account} has no accounts.{provider}.plans entry"));
            }
            let Some(hosts) = hosts.as_table_like() else { continue };
            for (host, profile) in hosts.iter() {
                let Some(path) = profile.get("path").and_then(Item::as_str) else { continue };
                if profiles.has_directory(host, path) == Some(false) {
                    found.push(format!("accounts.{provider}.{account} has no profile directory {path} on {host}"));
                }
                if let (Some(plan), Some(tier)) = (plan, profiles.tier(host, path))
                    && plan != tier
                {
                    found.push(format!("accounts.{provider}.{account} plans entry {plan} differs from the {tier} rate-limit tier of its profile on {host}"));
                }
            }
        }
    }
    found
}

