use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

const CREDENTIALS: &str = ".credentials.json";
const CONVERSATION_STATE: &str = ".claude.json";
const SETTINGS: &str = "settings.json";
const SIGN_IN: &str = "run `claude`, finish its sign-in and quit, then run this again";

pub fn profile(home: &Path, label: &str) -> Result<String, String> {
    let profile = home.join(".local/state/north/profiles").join(format!("claude-{label}"));
    if profile.join(CREDENTIALS).is_file() {
        return Ok(format!("claude-{label} already has a login in {}", profile.display()));
    }
    let login = home.join(".claude").join(CREDENTIALS);
    if !login.is_file() {
        return Err(format!("Claude Code keeps no login at {}; {SIGN_IN}", login.display()));
    }
    fs::create_dir_all(&profile).map_err(|error| at(&profile, &error))?;
    let linked = profile.join(CREDENTIALS);
    let _ = fs::remove_file(&linked);
    symlink(&login, &linked).map_err(|error| at(&linked, &error))?;
    let state = home.join(CONVERSATION_STATE);
    let copied = profile.join(CONVERSATION_STATE);
    if state.is_file() {
        fs::copy(&state, &copied).map_err(|error| at(&copied, &error))?;
    }
    let settings = profile.join(SETTINGS);
    if !settings.exists() {
        fs::write(&settings, "{}\n").map_err(|error| at(&settings, &error))?;
    }
    Ok(format!("made claude-{label} in {} from the login in {}", profile.display(), login.display()))
}

fn at(path: &Path, error: &std::io::Error) -> String {
    format!("{}: {error}", path.display())
}
