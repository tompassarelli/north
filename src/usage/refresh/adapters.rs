use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

use serde_json::Value;

use super::record::Reading;
use crate::usage::store::COLUMNS;

const ADAPTER_LIMIT: Duration = Duration::from_secs(30);
const REMOTE_LIMIT: Duration = Duration::from_secs(45);

pub(super) struct Readers<'a> {
    pub directory: PathBuf,
    pub machines: Vec<(String, Vec<String>)>,
    pub here: String,
    pub ssh: &'a dyn Fn(&[&str]) -> Vec<OsString>,
    pub run: &'a (dyn Fn(Command, Duration) -> Option<Output> + Sync),
}

impl<'a> Readers<'a> {
    pub(super) fn new(host: &'a crate::usage::Host, directory: PathBuf) -> Self {
        Self { directory, machines: crate::usage::settings::machines(), here: host.name.clone(), ssh: host.ssh, run: host.run }
    }

    pub(super) fn read_all(&self, remote: bool) -> Vec<Reading> {
        let adapters = self.adapters();
        let remotes: Vec<(&str, Command)> = self
            .machines
            .iter()
            .flat_map(|(provider, hosts)| hosts.iter().map(move |host| (provider.as_str(), host.as_str())))
            .filter(|(_, host)| remote && *host != self.here)
            .map(|(provider, host)| (provider, self.remote_command(host)))
            .collect();
        let (remote_out, local_out) = std::thread::scope(|scope| {
            let remote_runs: Vec<_> = remotes.into_iter().map(|(provider, command)| (provider, scope.spawn(|| (self.run)(command, REMOTE_LIMIT)))).collect();
            let local_runs: Vec<_> = adapters
                .into_iter()
                .map(|(name, path)| {
                    let mut command = Command::new("python3");
                    command.arg("-I").arg(path);
                    (name, scope.spawn(|| (self.run)(command, ADAPTER_LIMIT)))
                })
                .collect();
            let join = |run: std::thread::ScopedJoinHandle<'_, Option<Output>>| run.join().ok().flatten();
            (
                remote_runs.into_iter().map(|(provider, run)| (provider, join(run))).collect::<Vec<_>>(),
                local_runs.into_iter().map(|(name, run)| (name, join(run))).collect::<Vec<_>>(),
            )
        });
        let mut rows = Vec::new();
        for (provider, output) in remote_out {
            if let Some(output) = output {
                rows.extend(remote_rows(&output.stdout, provider));
            }
        }
        for (name, output) in local_out {
            match output {
                Some(output) => rows.extend(adapter_rows(&output.stdout)),
                None => eprintln!("agents usage: {name} adapter timed out after {} s", ADAPTER_LIMIT.as_secs()),
            }
        }
        rows
    }

    fn adapters(&self) -> Vec<(String, PathBuf)> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(&self.directory)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
                name.ends_with(".py") && (name.starts_with("usage_") || name.starts_with("billing_"))
            })
            .collect();
        found.sort();
        found
            .into_iter()
            .map(|path| {
                let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
                (stem.strip_prefix("usage_").unwrap_or(stem).to_owned(), path)
            })
            .collect()
    }

    fn remote_command(&self, host: &str) -> Command {
        let argv = (self.ssh)(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host, "agents", "usage", "--refresh", "--local", "--json"]);
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        command
    }
}

fn adapter_rows(stdout: &[u8]) -> Vec<Reading> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(row)) => Some(row),
            _ => None,
        })
        .collect()
}

fn remote_rows(stdout: &[u8], provider: &str) -> Vec<Reading> {
    let Ok(Value::Array(rows)) = serde_json::from_slice::<Value>(stdout) else { return Vec::new() };
    rows.into_iter()
        .filter_map(|row| row.as_object().filter(|row| row.get("provider").and_then(Value::as_str) == Some(provider)).cloned())
        .map(|row| COLUMNS.iter().map(|name| ((*name).to_owned(), row.get(*name).cloned().unwrap_or(Value::Null))).collect())
        .collect()
}
