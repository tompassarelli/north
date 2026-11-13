use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use north_base::verdict_cache;
use tokio::process::Command;
use tokio::task::JoinSet;

use super::{ACTIONS_CLASS, Shared, say};
use crate::land::slots::affected::PACKAGES_ENV;
use crate::land::slots::{lint_gate, listed_crates, verdict_key};
use crate::land::telemetry::epoch;
use crate::land::trace::{self, Category};

const LOCAL: &str = "local";

struct Inputs {
    root: PathBuf,
    base: String,
    tree: String,
    environment: Vec<(String, String)>,
    after: bool,
}

impl Inputs {
    fn of(shared: &Shared) -> Arc<Self> {
        let environment = shared.scope_environment().into_iter().map(|(name, value)| (name.to_owned(), value)).collect();
        Arc::new(Self { root: shared.root.clone(), base: shared.arguments.base.clone(), tree: shared.arguments.tree().to_owned(), environment, after: shared.arguments.after })
    }

    fn key(&self, command: &str) -> Option<String> {
        let selected = self.environment.iter().find(|(name, _)| name == PACKAGES_ENV).map(|(_, value)| value.as_str());
        if !self.after && listed_crates::decides(command, selected) {
            let (name, value) = listed_crates::BEFORE_LANDING;
            let environment: Vec<(String, String)> = self.environment.iter().cloned().chain([(name.to_owned(), value.to_owned())]).collect();
            return verdict_key::key(&self.root, &self.base, &self.tree, command, &environment);
        }
        if self.after && lint_gate::wraps(command) {
            let (name, value) = lint_gate::AFTER_LANDING;
            let environment: Vec<(String, String)> = self.environment.iter().cloned().chain([(name.to_owned(), value.to_owned())]).collect();
            return verdict_key::key(&self.root, &self.base, &self.tree, command, &environment);
        }
        verdict_key::key(&self.root, &self.base, &self.tree, command, &self.environment)
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

async fn north(program: &str, root: &Path, arguments: &[String]) -> Option<std::process::Output> {
    Command::new(program).args(arguments).current_dir(root).stdin(Stdio::null()).stderr(Stdio::null()).output().await.ok()
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

    /// Whether the `watched` commands passed in a run that exited `status`: all of them when it passed, else as their keyed verdicts say; `None` when nothing is watched.
    pub(super) async fn passed(&self, watched: &[String], status: i32) -> Option<bool> {
        if watched.is_empty() {
            return None;
        }
        Some(status == 0 || self.cached(watched).await.is_empty())
    }

    async fn lookup(&self, commands: &[String]) -> (Vec<String>, Vec<(String, String)>) {
        let inputs = Inputs::of(self);
        let land = keys(&inputs, commands).await;
        let found = self.hits(&land.values().map(String::as_str).collect::<Vec<_>>()).await;
        let mut pending = Vec::new();
        let mut reused = Vec::new();
        for command in commands {
            let Some((key, (host, pass))) = land.get(command).and_then(|key| found.get(key).map(|hit| (key.clone(), hit))) else {
                pending.push(command.clone());
                continue;
            };
            say(&format!("safe-push: {}", verdict_cache::reused_line(command, &key, host, pass)));
            reused.push((command.clone(), key));
        }
        (pending, reused)
    }

    async fn hits(&self, wanted: &[&str]) -> BTreeMap<String, (String, verdict_cache::Pass)> {
        if wanted.is_empty() {
            return BTreeMap::new();
        }
        match north(&self.environment.north, &self.root, &verdict_key::hit_arguments(wanted)).await {
            Some(output) if output.status.success() => verdict_key::hits(&String::from_utf8_lossy(&output.stdout)),
            _ => {
                say("safe-push: the verdict cache could not be read, so every land check runs");
                BTreeMap::new()
            }
        }
    }

    pub(super) async fn cache_record_as(&self, status: i32, commands: &[String], class: Option<&str>) {
        let inputs = Inputs::of(self);
        for (command, key) in keys(&inputs, commands).await {
            let class = class.map_or_else(|| self.cache_class(&command), str::to_owned);
            let host = if class == ACTIONS_CLASS { LOCAL } else { class.as_str() };
            let arguments = verdict_key::record_arguments(host, &key, (status == 0).then_some(inputs.tree.as_str()));
            let _ = north(&self.environment.north, &self.root, &arguments).await;
        }
    }

    pub(super) async fn cache_record(&self, status: i32, commands: &[String]) {
        self.cache_record_as(status, commands, None).await;
    }
}
