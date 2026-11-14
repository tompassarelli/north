use std::collections::BTreeSet;

const TOLD: &str = ".local/state/north/alarms-told.json";

/// Tells the coordinator once when the alarm under `key` starts (`text`) and once when it clears
/// (`None`), and files no issue, since an automatic filer's issue was refiled within the hour of
/// every close (north#1488). The keys told and not yet cleared stay in `TOLD` under the home directory.
pub fn tell_coordinator(key: &str, text: Option<&str>) -> Result<(), String> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).ok_or("HOME is not set")?;
    let path = std::path::PathBuf::from(home).join(TOLD);
    let mut told: BTreeSet<String> = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let message = match (told.contains(key), text) {
        (false, Some(text)) => format!("{text}\n(north files no issue for this alarm; you are told once when it starts and once when it clears)"),
        (true, None) => format!("cleared: {key}"),
        _ => return Ok(()),
    };
    let running = std::env::current_exe().ok().filter(|exe| exe.file_name().is_some_and(|name| name == "north"));
    let status = std::process::Command::new(running.unwrap_or_else(|| "north".into())).args(["fleet", "send", "--from", "service:north-alarm", "proxy"]).arg(&message).env_remove("NORTH_RUNTIME_LINK").stdin(std::process::Stdio::null()).status().map_err(|error| format!("cannot run north fleet send: {error}"))?;
    if !status.success() {
        return Err(format!("north fleet send proxy: {status}"));
    }
    if text.is_some() {
        told.insert(key.to_owned());
    } else {
        told.remove(key);
    }
    let failed = |error: std::io::Error| format!("{}: {error}", path.display());
    path.parent().map_or(Ok(()), std::fs::create_dir_all).map_err(failed)?;
    std::fs::write(&path, serde_json::to_string(&told).map_err(|error| error.to_string())? + "\n").map_err(failed)
}
