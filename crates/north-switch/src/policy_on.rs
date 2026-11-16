//! `north policy on`: puts back exactly what `north policy off` recorded on this host: each surface it moved aside,
//! the hooks and shell prefix it took out of `~/.claude/settings.json`, and each checkout's former git hooks. A
//! surface whose place was filled again meanwhile, or a checkout whose hooks were changed again, is left as found
//! and stays recorded, so nothing is overwritten.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use serde_json::Value;

use crate::paths::Paths;
use crate::policy::{self, Entry, Stash};
use crate::policy_off::{git, write_settings};

pub fn on() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("north policy on: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let paths = Paths::here()?;
    let stash = Stash::new(&paths.state_home);
    let entries = stash.read()?;
    if entries.is_empty() {
        println!("north policy on: north's policy is on; north policy off recorded nothing at {}", stash.record().display());
        return Ok(ExitCode::SUCCESS);
    }
    let mut kept = Vec::new();
    for entry in entries {
        match restore(&paths.home, &stash, &entry) {
            Ok(done) if done.is_empty() => {}
            Ok(done) => println!("north policy on: {done}"),
            Err(error) => {
                println!("north policy on: {error}");
                kept.push(entry);
            }
        }
    }
    if kept.is_empty() {
        stash.write(&[])?;
        println!("north policy on: north's hooks, skills, instructions and git checks are back for sessions and commits from now");
        return Ok(ExitCode::SUCCESS);
    }
    kept.insert(0, Entry::Off);
    stash.write(&kept)?;
    println!("north policy on: {} recorded changes could not be put back and stay in {}", kept.len() - 1, stash.record().display());
    Ok(ExitCode::from(1))
}

fn restore(home: &Path, stash: &Stash, entry: &Entry) -> Result<String, String> {
    match entry {
        Entry::Off => Ok(String::new()),
        Entry::Moved(surface) => restore_surface(home, stash, surface),
        Entry::Settings => restore_settings(home, stash),
        Entry::Hooks { repository, .. } if !policy::checkouts(home).contains(repository) => Err(format!("{} is not a checkout under ~/{}, so its recorded git hooks are not applied", repository.display(), policy::CHECKOUTS)),
        Entry::Hooks { repository, before } => rehook(repository, before.as_deref(), &stash.no_hooks()),
    }
}

fn restore_surface(home: &Path, stash: &Stash, surface: &str) -> Result<String, String> {
    let live = home.join(surface);
    let kept = stash.moved(surface);
    if fs::symlink_metadata(&live).is_ok() {
        return Err(format!("{} exists again, so the one north policy off moved stays at {}", live.display(), kept.display()));
    }
    fs::rename(&kept, &live).map(|()| format!("restored ~/{surface}")).map_err(|error| format!("{}: {error}", kept.display()))
}

fn restore_settings(home: &Path, stash: &Stash) -> Result<String, String> {
    let path = home.join(policy::SETTINGS);
    let read = |file: &Path| fs::read_to_string(file).map_err(|error| format!("{}: {error}", file.display())).and_then(|text| serde_json::from_str::<Value>(&text).map_err(|error| format!("{}: {error}", file.display())));
    let taken = read(&stash.settings())?;
    let mut settings = read(&path)?;
    let object = settings.as_object_mut().ok_or_else(|| format!("{} is not a JSON object", path.display()))?;
    if let Some(hooks) = taken.get("hooks") {
        object.entry("hooks").or_insert_with(|| hooks.clone());
    }
    if let Some(prefix) = taken.get(policy::SHELL_PREFIX) {
        let env = object.entry("env").or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Some(env) = env.as_object_mut() {
            env.entry(policy::SHELL_PREFIX).or_insert_with(|| prefix.clone());
        }
    }
    write_settings(&path, &settings)?;
    fs::remove_file(stash.settings()).map_err(|error| format!("{}: {error}", stash.settings().display()))?;
    Ok(format!("put north's hooks and worker-sandbox shell prefix back into ~/{}", policy::SETTINGS))
}

fn rehook(repository: &Path, before: Option<&str>, no_hooks: &Path) -> Result<String, String> {
    let now = git(repository, &["config", "--local", "--get", "core.hooksPath"]).ok();
    if now.as_deref() != Some(&*no_hooks.to_string_lossy()) {
        return Err(format!("{} has git hooks set to {} since north policy off, so they are left as they are", repository.display(), now.as_deref().unwrap_or("git's default")));
    }
    match before {
        Some(path) => git(repository, &["config", "--local", "core.hooksPath", path]),
        None => git(repository, &["config", "--local", "--unset", "core.hooksPath"]),
    }
    .map(|_| String::new())
}
