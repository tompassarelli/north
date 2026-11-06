use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use north_base::verdict_cache::{self, Check, Pass};
use tokio::process::Command;
use tokio::task::JoinSet;

use super::{ACTIONS_CLASS, Shared, say};
use crate::land::after::follow::program;
use crate::land::telemetry::epoch;
use crate::land::trace::{self, Category};

const LOCAL: &str = "local";

struct Inputs {
    root: PathBuf,
    tree: String,
    runtime: String,
    environment: Vec<(String, String)>,
}

impl Inputs {
    fn of(shared: &Shared) -> Arc<Self> {
        let tree = shared.arguments.tree();
        let mut environment: Vec<(String, String)> = [("SAFE_PUSH_LAND_BASE", shared.arguments.base.as_str()), ("SAFE_PUSH_LAND_HEAD", tree), ("SAFE_PUSH_LAND_RUNTIME_HEAD", tree)].into_iter().map(|(name, value)| (name.to_owned(), value.to_owned())).collect();
        environment.extend(shared.scope_environment().into_iter().map(|(name, value)| (name.to_owned(), value)));
        Arc::new(Self { root: shared.root.clone(), tree: tree.to_owned(), runtime: verdict_cache::runtime_identity(), environment })
    }

    fn key(&self, command: &str) -> Option<String> {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let (south, nixos) = (home.join("code/south/main"), home.join("code/nixos-config/main"));
        let machine_repos: [(&str, &Path); 2] = [("south", &south), ("nixos-config", &nixos)];
        verdict_cache::key(&Check { repo: &self.root, tree: &self.tree, command, runtime: &self.runtime, environment: &self.environment, machine_repos: &machine_repos })
    }
}

async fn keys(inputs: &Arc<Inputs>, commands: &[String]) -> BTreeMap<String, String> {
    let mut work = JoinSet::new();
    for command in commands {
        let (inputs, command) = (Arc::clone(inputs), command.clone());
        work.spawn_blocking(move || inputs.key(&command).map(|key| (command, key)));
    }
    let mut keys = BTreeMap::new();
    while let Some(joined) = work.join_next().await {
        keys.extend(joined.ok().flatten());
    }
    keys
}

async fn north(root: &Path, arguments: &[&str]) -> Option<std::process::Output> {
    Command::new(program()).args(arguments).current_dir(root).stdin(Stdio::null()).stderr(Stdio::null()).output().await.ok()
}

fn hits(text: &str) -> BTreeMap<String, (String, Pass)> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split(' ');
            let (key, host, seconds, sha) = (words.next()?, words.next()?, words.next()?, words.next()?);
            Some((key.to_owned(), (host.to_owned(), Pass::parse(&format!("{seconds} {sha}"))?)))
        })
        .collect()
}

impl Shared {
    pub(super) async fn reuse(&self, commands: Vec<String>) -> Vec<String> {
        let looked = epoch();
        let (pending, reused) = self.lookup(&commands).await;
        for (command, key) in reused {
            trace::record(&[self.span(&command, Category::of_command(&command), looked).with("cache", "hit").with(trace::REUSED, format!("reused verdict {key}"))]);
        }
        pending
    }

    pub(super) async fn cached(&self, commands: &[String]) -> Vec<String> {
        self.lookup(commands).await.0
    }

    async fn lookup(&self, commands: &[String]) -> (Vec<String>, Vec<(String, String)>) {
        let keys = keys(&Inputs::of(self), commands).await;
        let mut arguments = vec!["capacity", "offload", "verdict", "hit"];
        arguments.extend(keys.values().map(String::as_str));
        let found = if keys.is_empty() {
            BTreeMap::new()
        } else {
            match north(&self.root, &arguments).await {
                Some(output) if output.status.success() => hits(&String::from_utf8_lossy(&output.stdout)),
                _ => {
                    say("safe-push: the verdict cache could not be read, so every land check runs");
                    BTreeMap::new()
                }
            }
        };
        let mut pending = Vec::new();
        let mut reused = Vec::new();
        for command in commands {
            match keys.get(command).and_then(|key| found.get(key).map(|hit| (key, hit))) {
                Some((key, (host, pass))) => {
                    say(&format!("safe-push: {}", verdict_cache::reused_line(command, key, host, pass)));
                    reused.push((command.clone(), key.clone()));
                }
                None => pending.push(command.clone()),
            }
        }
        (pending, reused)
    }

    pub(super) async fn cache_record_as(&self, status: i32, commands: &[String], class: Option<&str>) {
        let inputs = Inputs::of(self);
        for (command, key) in keys(&inputs, commands).await {
            let class = class.map_or_else(|| self.cache_class(&command), str::to_owned);
            let host = if class == ACTIONS_CLASS { LOCAL } else { class.as_str() };
            let verdict = if status == 0 { ["--pass", inputs.tree.as_str()] } else { ["--fail", ""] };
            let mut arguments = vec!["capacity", "offload", "verdict", "record", "--host", host, "--key", key.as_str()];
            arguments.extend(verdict.into_iter().filter(|word| !word.is_empty()));
            let _ = north(&self.root, &arguments).await;
        }
    }

    pub(super) async fn cache_record(&self, status: i32, commands: &[String]) {
        self.cache_record_as(status, commands, None).await;
    }
}
