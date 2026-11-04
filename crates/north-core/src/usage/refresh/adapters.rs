use std::ffi::OsString;
use std::process::{Command, Output};
use std::time::Duration;

use serde_json::Value;

use super::record::Reading;
use super::{agy, claude, digitalocean};
use crate::usage::store::COLUMNS;

const REMOTE_LIMIT: Duration = Duration::from_secs(45);

pub(super) type Local<'a> = Box<dyn Fn() -> Vec<Reading> + Send + Sync + 'a>;

pub(super) struct Readers<'a> {
    pub local: Vec<Local<'a>>,
    pub machines: Vec<(String, Vec<String>)>,
    pub here: String,
    pub ssh: &'a dyn Fn(&[&str]) -> Vec<OsString>,
    pub run: &'a (dyn Fn(Command, Duration) -> Option<Output> + Sync),
}

impl<'a> Readers<'a> {
    pub(super) fn new(host: &'a crate::usage::Host) -> Self {
        let billing = || {
            digitalocean::refresh();
            Vec::new()
        };
        let local: Vec<Local<'a>> = vec![Box::new(billing), Box::new(agy::read), Box::new(|| claude::read(&claude::Settings::from_env(true)))];
        Self { local, machines: crate::usage::settings::machines(), here: host.name.clone(), ssh: host.ssh, run: host.run }
    }

    pub(super) fn read_all(&self, remote: bool) -> Vec<Reading> {
        let remotes: Vec<(&str, Command)> = self
            .machines
            .iter()
            .flat_map(|(provider, hosts)| hosts.iter().map(move |host| (provider.as_str(), host.as_str())))
            .filter(|(_, host)| remote && *host != self.here)
            .map(|(provider, host)| (provider, self.remote_command(host)))
            .collect();
        let (remote_out, local_out) = std::thread::scope(|scope| {
            let remote_runs: Vec<_> = remotes.into_iter().map(|(provider, command)| (provider, scope.spawn(|| (self.run)(command, REMOTE_LIMIT)))).collect();
            let local_runs: Vec<_> = self.local.iter().map(|read| scope.spawn(read)).collect();
            (
                remote_runs.into_iter().map(|(provider, run)| (provider, run.join().ok().flatten())).collect::<Vec<_>>(),
                local_runs.into_iter().map(|run| run.join().unwrap_or_default()).collect::<Vec<_>>(),
            )
        });
        let mut rows = Vec::new();
        for (provider, output) in remote_out {
            if let Some(output) = output {
                rows.extend(remote_rows(&output.stdout, provider));
            }
        }
        for found in local_out {
            rows.extend(found);
        }
        rows
    }

    fn remote_command(&self, host: &str) -> Command {
        let argv = (self.ssh)(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host, "agents", "usage", "--refresh", "--local", "--json"]);
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        command
    }
}

fn remote_rows(stdout: &[u8], provider: &str) -> Vec<Reading> {
    let Ok(Value::Array(rows)) = serde_json::from_slice::<Value>(stdout) else { return Vec::new() };
    rows.into_iter()
        .filter_map(|row| row.as_object().filter(|row| row.get("provider").and_then(Value::as_str) == Some(provider)).cloned())
        .map(|row| COLUMNS.iter().map(|name| ((*name).to_owned(), row.get(*name).cloned().unwrap_or(Value::Null))).collect())
        .collect()
}
