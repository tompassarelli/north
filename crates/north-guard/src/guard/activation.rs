use crate::foundation::activation::activation_paths;
use rustix::fs::{Access, access};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

#[cfg(not(test))]
const OPERATOR_FILE: &str = "/etc/north/operator";
#[cfg(test)]
const OPERATOR_FILE: &str = "/nonexistent/north/operator";
const USERS_FILE: &str = "/etc/passwd";
const RELOCATING: [&str; 3] = ["NORTH_AGENT_STATE_ROOT", "NORTH_AGENT_ACTIVATION", "NORTH_AGENT_ACTIVE"];

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

fn digest(value: &serde_json::Value) -> bool {
    value
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

fn operator(named: &Path, users: &Path) -> bool {
    let name = match std::fs::read_to_string(named) {
        Ok(text) => text.trim().to_owned(),
        Err(error) => return error.kind() == std::io::ErrorKind::NotFound,
    };
    let uid = std::fs::read_to_string(users).ok().and_then(|users| {
        users.lines().find_map(|line| {
            let mut fields = line.split(':');
            (fields.next() == Some(name.as_str())).then(|| fields.nth(1)?.parse::<u32>().ok()).flatten()
        })
    });
    uid == Some(rustix::process::getuid().as_raw())
}

fn foreign(step: &Path) -> bool {
    std::fs::metadata(step).is_ok_and(|own| own.uid() != rustix::process::getuid().as_raw())
}

fn closed(step: &Path) -> bool {
    access(step, Access::WRITE_OK).is_err() && foreign(step)
}

fn sealed(record: &Path) -> bool {
    let (Ok(given), Ok(real)) = (std::path::absolute(record), std::fs::canonicalize(record)) else {
        return false;
    };
    [given, real].iter().all(|path| path.ancestors().all(closed))
}

fn trusted(record: Option<&Path>) -> bool {
    operator(Path::new(OPERATOR_FILE), Path::new(USERS_FILE)) || record.is_some_and(sealed)
}

fn override_answer(get: &impl Fn(&str) -> Option<String>, trusted: &impl Fn(Option<&Path>) -> bool) -> Option<bool> {
    let value = get("AGENT_NO_AUTHORING_HOOKS").filter(|s| !s.is_empty())?;
    let off = !matches!(value.as_str(), "0" | "false");
    (!off || trusted(None)).then_some(off)
}

fn hook_id(id: &str, get: &impl Fn(&str) -> Option<String>, trusted: &impl Fn(Option<&Path>) -> bool) -> String {
    get("NORTH_HOOK_ID").filter(|_| trusted(None)).unwrap_or_else(|| id.to_owned())
}

fn login_home(users: &Path) -> Option<String> {
    let uid = rustix::process::getuid().as_raw();
    std::fs::read_to_string(users).ok()?.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.nth(2)?.parse::<u32>().ok()? == uid).then(|| fields.nth(2).map(str::to_owned)).flatten()
    })
}

fn locations(get: &impl Fn(&str) -> Option<String>, trusted: &impl Fn(Option<&Path>) -> bool, home: &impl Fn() -> Option<String>) -> (std::path::PathBuf, std::path::PathBuf) {
    let login = if trusted(None) { None } else { Some(home().unwrap_or_else(|| "/nonexistent".to_owned())) };
    activation_paths(&|name: &str| match &login {
        Some(own) if name == "HOME" => Some(own.clone()),
        Some(_) if RELOCATING.contains(&name) => None,
        _ => get(name),
    })
}

fn guards_off(id: &str, get: &impl Fn(&str) -> Option<String>, trusted: &impl Fn(Option<&Path>) -> bool, home: &impl Fn() -> Option<String>) -> bool {
    if let Some(answer) = override_answer(get, trusted) {
        return answer;
    }
    let id = hook_id(id, get, trusted);
    let id = id.strip_suffix(".sh").unwrap_or(&id);
    let id = id.strip_suffix(".js").unwrap_or(id);
    let (list, path) = locations(get, trusted, home);
    if list.is_file()
        && let Ok(contents) = std::fs::read(&list) {
            let wanted = format!("hook {id}");
            return trusted(Some(&list))
                && !contents
                    .split(|b| *b == b'\n')
                    .any(|line| line.iter().copied().filter(|b| *b != 0).eq(wanted.bytes()));
        }
    let Some(data) = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    else {
        return false;
    };
    if data["schema"] != "north.agent-activation/v1"
        || !digest(&data["catalogDigest"])
        || !digest(&data["generationId"])
    {
        return false;
    }
    let Some(units) = data["units"].as_array() else {
        return false;
    };
    let matches: Vec<_> = units
        .iter()
        .filter(|u| u["kind"] == "hook" && u["id"] == id)
        .collect();
    trusted(Some(&path)) && (matches.len() != 1 || matches[0]["permission"] != "on" || matches[0]["active"] != true)
}

#[cfg(test)]
thread_local! {
    pub static TEST_GUARDS_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn authoring_guards_off(id: &str) -> bool {
    #[cfg(test)]
    if TEST_GUARDS_OFF.with(|off| off.get()) {
        return true;
    }
    guards_off_in(id, &nonempty_env)
}

pub fn guards_off_in(id: &str, get: &impl Fn(&str) -> Option<String>) -> bool {
    guards_off(id, get, &trusted, &|| login_home(Path::new(USERS_FILE)))
}

#[cfg(test)]
#[path = "activation_tests.rs"]
mod tests;
