use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use north_base::foundation::clock::Timestamp;

const USAGE: &str = "usage: north lane stray PATH...

Removes each PATH, a file git reports untracked (not ignored), only when this session's own
transcript holds a tool call naming PATH from before the file was born: a stray the session left
itself, such as a failed edit's temporary copy in a primary checkout. Anything else is kept and
the reason named.";

const RECORDED_SLACK: Duration = Duration::from_secs(5);

pub fn run(arguments: &[String]) -> u8 {
    if arguments.is_empty() || arguments.iter().any(|argument| argument.starts_with('-')) {
        eprintln!("{USAGE}");
        return 2;
    }
    let transcript = match own_transcript() {
        Ok(transcript) => transcript,
        Err(error) => {
            eprintln!("lane stray: {error}");
            return 1;
        }
    };
    let mut code = 0;
    for path in arguments.iter().map(PathBuf::from) {
        match remove(&path, &transcript) {
            Ok(()) => println!("removed {}: an untracked file this session's own tool call created", path.display()),
            Err(reason) => {
                eprintln!("kept {}: {reason}", path.display());
                code = 1;
            }
        }
    }
    code
}

pub fn remove(path: &Path, transcript: &str) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("name it by its absolute path, as the session's tool call did".to_owned());
    }
    let meta = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !meta.file_type().is_file() {
        return Err("not a regular file".to_owned());
    }
    let parent = path.parent().ok_or("has no parent directory")?;
    let status = Command::new("git").arg("-C").arg(parent).args(["status", "--porcelain", "--ignored", "--untracked-files=all", "--"]).arg(path).output().map_err(|error| error.to_string())?;
    if !status.status.success() || !String::from_utf8_lossy(&status.stdout).starts_with("?? ") {
        return Err("git does not report it as an untracked file".to_owned());
    }
    let born = meta.created().or_else(|_| meta.modified()).map_err(|error| error.to_string())?;
    let named = first_named(transcript, &path.to_string_lossy()).ok_or("no tool call of this session names it")?;
    if born + RECORDED_SLACK < named {
        return Err("it existed before this session's first tool call naming it".to_owned());
    }
    std::fs::remove_file(path).map_err(|error| error.to_string())
}

fn first_named(transcript: &str, path: &str) -> Option<SystemTime> {
    transcript.lines().filter(|line| line.contains(path)).find_map(|line| {
        let entry: serde_json::Value = serde_json::from_str(line).ok()?;
        let calls = entry["message"]["content"].as_array()?;
        calls.iter().any(|block| block["type"] == "tool_use" && block["input"].to_string().contains(path)).then_some(())?;
        let at = Timestamp::parse_rfc3339(entry["timestamp"].as_str()?)?;
        Some(SystemTime::UNIX_EPOCH + Duration::from_millis(u64::try_from(at.unix_millis()).ok()?))
    })
}

fn own_transcript() -> Result<String, String> {
    let session = std::env::var("CLAUDE_CODE_SESSION_ID").map_err(|_| "CLAUDE_CODE_SESSION_ID is unset, so this session's transcript cannot be found".to_owned())?;
    let config = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude"))).ok_or("neither CLAUDE_CONFIG_DIR nor HOME is set")?;
    let name = format!("{session}.jsonl");
    let found = std::fs::read_dir(config.join("projects")).into_iter().flatten().flatten().map(|project| project.path().join(&name)).find(|candidate| candidate.is_file());
    let transcript = found.ok_or_else(|| format!("no transcript {name} under {}", config.join("projects").display()))?;
    std::fs::read_to_string(&transcript).map_err(|error| format!("cannot read {}: {error}", transcript.display()))
}
