//! `north policy off`: on this host, move north's instruction, skill and hook-script surfaces into the stash, take
//! north's hooks and worker-sandbox shell prefix out of `~/.claude/settings.json`, and point every checkout's git
//! hooks at an empty directory, so agent sessions started after it and the owner's own commits and pushes meet no
//! north policy. Nothing is deleted and nothing is stopped; `north policy on` puts back exactly what it recorded.

use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

use serde_json::{Map, Value};

use crate::paths::Paths;
use crate::policy::{self, Entry, Stash};

pub fn off() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("north policy off: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let paths = Paths::here()?;
    let stash = Stash::new(&paths.state_home);
    if stash.is_off() {
        println!("north policy off: already off since {}; `north policy on` restores it", stash.record().display());
        return Ok(ExitCode::SUCCESS);
    }
    fs::create_dir_all(stash.no_hooks()).map_err(|error| format!("{}: {error}", stash.no_hooks().display()))?;
    let mut entries = vec![Entry::Off];
    let mut problems = Vec::new();
    let mut keep = |taken: Result<Option<Entry>, String>| match taken {
        Ok(entry) => entries.extend(entry),
        Err(error) => problems.push(error),
    };
    for surface in policy::SURFACES {
        keep(move_aside(&paths.home, &stash, surface).map(|moved| moved.then(|| Entry::Moved(surface.to_owned()))));
    }
    keep(strip_settings(&paths.home, &stash).map(|stripped| stripped.then_some(Entry::Settings)));
    for repository in policy::checkouts(&paths.home) {
        keep(unhook(&repository, &stash.no_hooks()).map(|before| Some(Entry::Hooks { repository, before })));
    }
    stash.write(&entries)?;
    report(&entries, &problems);
    Ok(if problems.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

fn move_aside(home: &Path, stash: &Stash, surface: &str) -> Result<bool, String> {
    let live = home.join(surface);
    if fs::symlink_metadata(&live).is_err() {
        return Ok(false);
    }
    let kept = stash.moved(surface);
    if fs::symlink_metadata(&kept).is_ok() {
        return Err(format!("{} is left in place: {} already holds an earlier copy", live.display(), kept.display()));
    }
    let parent = kept.parent().unwrap_or(&stash.root);
    fs::create_dir_all(parent).and_then(|()| fs::rename(&live, &kept)).map(|()| true).map_err(|error| format!("{}: {error}", live.display()))
}

fn strip_settings(home: &Path, stash: &Stash) -> Result<bool, String> {
    let path = home.join(policy::SETTINGS);
    let Ok(text) = fs::read_to_string(&path) else { return Ok(false) };
    let mut settings: Value = serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    let Some(object) = settings.as_object_mut() else { return Ok(false) };
    let mut taken = Map::new();
    if let Some(hooks) = object.remove("hooks") {
        taken.insert("hooks".to_owned(), hooks);
    }
    if let Some(prefix) = object.get_mut("env").and_then(Value::as_object_mut).and_then(|env| env.remove(policy::SHELL_PREFIX)) {
        taken.insert(policy::SHELL_PREFIX.to_owned(), prefix);
    }
    if taken.is_empty() {
        return Ok(false);
    }
    fs::write(stash.settings(), Value::Object(taken).to_string()).map_err(|error| format!("{}: {error}", stash.settings().display()))?;
    write_settings(&path, &settings)?;
    Ok(true)
}

pub(crate) fn write_settings(path: &Path, settings: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    let real = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let staged = real.with_extension("json.north-policy");
    fs::write(&staged, text + "\n").and_then(|()| fs::rename(&staged, &real)).map_err(|error| format!("{}: {error}", real.display()))
}

fn unhook(repository: &Path, no_hooks: &Path) -> Result<Option<String>, String> {
    let before = git(repository, &["config", "--local", "--get", "core.hooksPath"]).ok();
    git(repository, &["config", "--local", "core.hooksPath", &no_hooks.to_string_lossy()])?;
    Ok(before)
}

pub(crate) fn git(repository: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git").arg("-C").arg(repository).args(arguments).output().map_err(|error| format!("git in {}: {error}", repository.display()))?;
    match output.status.success() {
        true => Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned()),
        false => Err(format!("git {} in {}: {}", arguments.join(" "), repository.display(), String::from_utf8_lossy(&output.stderr).trim())),
    }
}

fn report(entries: &[Entry], problems: &[String]) {
    let moved: Vec<&str> = entries.iter().filter_map(|entry| if let Entry::Moved(path) = entry { Some(path.as_str()) } else { None }).collect();
    let unhooked = entries.iter().filter(|entry| matches!(entry, Entry::Hooks { .. })).count();
    if !moved.is_empty() {
        println!("north policy off: moved aside north's instructions, skills and hook scripts: {}", moved.join(" "));
    }
    if entries.contains(&Entry::Settings) {
        println!("north policy off: took north's hooks and worker-sandbox shell prefix out of ~/{}", policy::SETTINGS);
    }
    println!("north policy off: {unhooked} checkouts under ~/{} run no git hooks, so a commit or push goes as plain git", policy::CHECKOUTS);
    for problem in problems {
        println!("north policy off: {problem}");
    }
    println!("north policy off: agent sessions started from now meet no north policy; `north policy on` restores it");
}
