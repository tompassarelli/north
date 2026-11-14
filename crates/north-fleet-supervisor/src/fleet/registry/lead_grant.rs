//! What the fleet tick runs for a confined lead's spooled launch and revive (north#1137): a lead in the worker domain cannot write the fleet's records, so its own `north fleet launch` and `north fleet revive` wait in its spool. The asker is the session whose spool carried the row, the one spool location its domain lets it write, and it is served only when its record, which the launcher wrote outside every domain, says role lead and names a task; neither its environment nor a word of its request says so. `fleet launch` starts a claude worker with that lead as its parent, in the main checkout of the lead's own project, from a closed list of words with the brief as text; `fleet revive NAME` revives only a stopped claude child of that lead. The tick opens no file a request names.

use std::path::{Path, PathBuf};

use crate::fleet::model::{Role, SessionRecord};

use super::Fleet;

const VALUED: [&str; 11] = ["--name", "--task", "--box", "--cwd", "--provider", "--model", "--effort", "--route", "--tier-reason", "--idle-exit", "--priority"];
const BARE: [&str; 2] = ["--wait", "--no-wait"];
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
const LAUNCH_SHAPE: &str = "a lead launches with north fleet launch --name N --task REF [--box TEXT] [--cwd PROJECT/main] [--provider claude] [--model M] [--effort LEVEL] [--route SPEC] [--tier-reason TEXT] [--idle-exit MINUTES] [--priority normal] [--wait | --no-wait] BRIEF...";
const REVIVE_SHAPE: &str = "a lead revives with north fleet revive NAME [--effort LEVEL]";

/// What the tick runs for a lead: `north fleet ARGUMENTS`, which starts or revives the session `name`, in `cwd`, the main checkout of the lead's own project.
#[derive(Debug, PartialEq)]
pub struct Granted {
    pub name: String,
    pub arguments: Vec<String>,
    pub cwd: PathBuf,
}

/// The lead that asks: its record and the main checkout of the project it runs in.
struct Lead {
    record: SessionRecord,
    main: PathBuf,
}

impl Fleet {
    /// The `north fleet launch` the tick runs for the spooled launch `words` of `writer`, or why it is refused.
    pub fn granted_launch(&self, writer: &str, words: &[String]) -> Result<Granted, String> {
        let lead = self.lead(writer)?;
        let (name, arguments) = launch(&lead, words)?;
        Ok(Granted { name, arguments, cwd: lead.main })
    }

    /// The `north fleet revive` the tick runs for the spooled revive `words` of `writer`, or why it is refused.
    pub fn granted_revive(&self, writer: &str, words: &[String]) -> Result<Granted, String> {
        let lead = self.lead(writer)?;
        let arguments = self.revive_arguments(&lead, words)?;
        Ok(Granted { name: arguments[1].clone(), arguments, cwd: lead.main })
    }

    /// Why the spooled hold of `task` by `writer` is refused, or None when `writer` is a lead with a task and `task` is an issue of its own project.
    pub(super) fn refused_hold(&self, writer: &str, task: &str) -> Option<String> {
        self.lead(writer).and_then(|lead| own_project(&lead, task)).err()
    }

    /// Why a spooled control row of `writer` is refused, or None when its record says role lead and names a task.
    pub(super) fn refused_writer(&self, writer: &str) -> Option<String> {
        self.lead(writer).err()
    }

    fn lead(&self, writer: &str) -> Result<Lead, String> {
        let record = self.record(writer).map_err(|_| format!("{writer} has no session record on this host, so its spool carries no fleet verb here"))?;
        if record.role() != Role::Lead {
            return Err(format!("{writer} runs as a {}, and only a lead's spool carries fleet launch, revive, hold and forget; its parent restaffs it", record.role().label()));
        }
        if record.task_ref.is_none() {
            return Err(format!("{writer}'s record holds no task: a lead with an empty record runs unconfined and writes the fleet's records itself, so its launch is not granted"));
        }
        let main = own_main(writer, &record.cwd, &self.home.join("code"))?;
        Ok(Lead { record, main })
    }

    fn revive_arguments(&self, lead: &Lead, words: &[String]) -> Result<Vec<String>, String> {
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        let (name, effort) = match words.as_slice() {
            [name] => (*name, None),
            [name, "--effort", level] if EFFORTS.contains(level) => (*name, Some(*level)),
            _ => return Err(format!("fleet revive {} is not granted: {REVIVE_SHAPE}", words.join(" "))),
        };
        if !session_name(name) {
            return Err(format!("{name} is not a session name; {REVIVE_SHAPE}"));
        }
        let child = self.record(name).map_err(|_| format!("no fleet session named {name} is recorded on this host"))?;
        let parent = child.parent.as_deref().unwrap_or("none");
        if parent != lead.record.name {
            return Err(format!("{name}'s recorded parent is {parent}, not {}; a lead revives only its own children", lead.record.name));
        }
        if child.live() {
            return Err(format!("{name} is live; a lead revives only a child that has stopped"));
        }
        placed_again(&child)?;
        Ok(["revive", name].into_iter().chain(effort.into_iter().flat_map(|level| ["--effort", level])).map(str::to_owned).collect())
    }
}

/// The main checkout of the project the lead `name` runs in from `cwd`: `CODE/PROJECT/main` beside its lane or its main checkout.
fn own_main(name: &str, cwd: &Path, code: &Path) -> Result<PathBuf, String> {
    let project = match cwd.parent() {
        Some(parent) if parent.file_name().is_some_and(|directory| directory == "worktrees") => parent.parent(),
        Some(parent) if cwd.file_name().is_some_and(|directory| directory == "main") => Some(parent),
        _ => None,
    };
    project.filter(|project| project.parent() == Some(code)).map(|project| project.join("main")).ok_or_else(|| format!("{name} runs in {}, which is no checkout of a project under {}", cwd.display(), code.display()))
}

/// The name of the worker `lead` launches with `words` and the arguments after `north fleet` that launch it.
fn launch(lead: &Lead, words: &[String]) -> Result<(String, Vec<String>), String> {
    let mut granted: Vec<String> = ["launch", "--provider", "claude", "--role", "worker", "--parent", &lead.record.name].map(str::to_owned).to_vec();
    let mut named: Vec<(&str, &str)> = Vec::new();
    let mut at = 0;
    while let Some(word) = words.get(at).map(String::as_str).filter(|word| word.starts_with("--")) {
        if named.iter().any(|(flag, _)| *flag == word) {
            return Err(format!("fleet launch {word} is given twice; {LAUNCH_SHAPE}"));
        }
        if BARE.contains(&word) {
            named.push((word, ""));
            granted.push(word.to_owned());
            at += 1;
            continue;
        }
        let value = flag_value(lead, word, words.get(at + 1).map(String::as_str))?;
        named.push((word, value));
        if !matches!(word, "--provider" | "--cwd") {
            granted.extend([word.to_owned(), value.to_owned()]);
        }
        at += 2;
    }
    let value = |flag: &str| named.iter().find(|(named, _)| *named == flag).map(|(_, value)| (*value).to_owned());
    let (Some(name), Some(_)) = (value("--name"), value("--task")) else {
        return Err(format!("a lead names the worker it launches and the task it holds; {LAUNCH_SHAPE}"));
    };
    let brief = words[at..].join(" ");
    if brief.trim().is_empty() {
        return Err(format!("a lead's launch carries the brief as text after its flags; {LAUNCH_SHAPE}"));
    }
    granted.extend(["--cwd".to_owned(), lead.main.display().to_string(), brief]);
    Ok((name, granted))
}

/// The value `value` the launch flag `word` carries, or why the flag or its value is not granted to `lead`.
fn flag_value<'a>(lead: &Lead, word: &str, value: Option<&'a str>) -> Result<&'a str, String> {
    if !VALUED.contains(&word) {
        return Err(format!("fleet launch {word} is not granted to a lead: the parent is the lead itself, the role is worker, the account is the placement step's and no file is opened; {LAUNCH_SHAPE}"));
    }
    let value = value.filter(|value| !value.starts_with("--") && !value.trim().is_empty()).ok_or_else(|| format!("fleet launch {word} needs a value; {LAUNCH_SHAPE}"))?;
    let refusal = match word {
        "--provider" => (value != "claude").then(|| format!("fleet launch --provider {value} is not granted to a lead: while the API hold stands a lead launches claude subscription workers only")),
        "--priority" => (value != "normal").then(|| format!("fleet launch --priority {value} is not granted to a lead: only the coordinator grants urgent priority")),
        "--model" | "--route" => names_api(value).then(|| format!("fleet launch {word} {value} names an API organization or key, which no lead draws while the API hold stands")),
        "--effort" => (!EFFORTS.contains(&value)).then(|| format!("fleet launch --effort {value} is not one of {}", EFFORTS.join(", "))),
        "--idle-exit" => (!value.bytes().all(|byte| byte.is_ascii_digit())).then(|| format!("fleet launch --idle-exit {value} is not a number of minutes")),
        "--name" => (!session_name(value)).then(|| format!("fleet launch --name {value} is not a session name")),
        "--task" => own_task(lead, value).err(),
        "--cwd" => own_checkout(lead, value).err(),
        _ => None,
    };
    refusal.map_or(Ok(value), Err)
}

/// Refuses a `--task` that is no issue of the lead's own project or that its record does not hold, owner prefix aside.
fn own_task(lead: &Lead, value: &str) -> Result<(), String> {
    own_project(lead, value)?;
    let issue = |task: &str| task.rsplit('/').next().unwrap_or(task).to_owned();
    if !lead.record.tasks().any(|held| issue(held) == issue(value)) {
        return Err(format!("fleet launch --task {value} is not granted to {}: a lead launches workers only for an issue its own record holds ({})", lead.record.name, lead.record.tasks().collect::<Vec<_>>().join(", ")));
    }
    Ok(())
}

/// Refuses a task that is not `[OWNER/]REPO#N` in the lead's own project, the directory under `code/` its main checkout sits in: placement takes a launch's repository from its task first, and the repository picks the login and the surface, so a lead names no other project's issue.
fn own_project(lead: &Lead, task: &str) -> Result<(), String> {
    let project = lead.main.parent().and_then(Path::file_name).map(|name| name.to_string_lossy().to_lowercase()).unwrap_or_default();
    let named = task.split_once('#').filter(|(_, number)| number.starts_with(|c: char| c.is_ascii_digit())).map(|(repository, _)| repository.rsplit('/').next().unwrap_or(repository).trim_end_matches(".git").to_lowercase());
    if named.as_deref() != Some(project.as_str()) {
        return Err(format!("{task} is not an issue of {project}, the project {} runs in; a lead holds and launches for its own project's issues only", lead.record.name));
    }
    Ok(())
}

/// Refuses a `--cwd` other than the main checkout of the lead's own project, written as its canonical path: a child launched there gets that checkout's write domain, so a lead gives it no other project's, and the text names no `..` and passes no link.
fn own_checkout(lead: &Lead, value: &str) -> Result<(), String> {
    let canonical = Path::new(value).canonicalize().ok().is_some_and(|real| real.as_os_str() == value);
    if !canonical || Path::new(value) != lead.main {
        return Err(format!("fleet launch --cwd {value} is not granted to {}: a lead launches its workers in {}, its own project's main checkout, named by its canonical path", lead.record.name, lead.main.display()));
    }
    Ok(())
}

/// Refuses the revive of `child` unless the revive places it again through the spend rule and the API hold: `fleet revive` re-runs placement only for a claude session that is no verification session, and an `api` account is never a lead's to draw.
fn placed_again(child: &SessionRecord) -> Result<(), String> {
    let label = child.account.strip_prefix("claude:").unwrap_or(&child.account);
    let refusal = if child.provider.label() != "claude" {
        Some(format!("{} runs on {}, whose revive is not placed again, so a lead revives only claude children", child.name, child.provider.label()))
    } else if child.verification {
        Some(format!("{} is a verification session, whose revive is not placed again", child.name))
    } else if label == "api" || label.starts_with("api:") {
        Some(format!("{} runs on the API account {}, which no lead draws while the API hold stands", child.name, child.account))
    } else {
        None
    };
    refusal.map_or(Ok(()), Err)
}

fn session_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with(['.', '-']) && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

fn names_api(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.split([',', ':', '/']).any(|part| part == "api") || lower.contains("sk-")
}
