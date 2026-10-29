use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) fn registration_files(home: &Path, state: &Path) -> Vec<PathBuf> {
    let mut files = vec![home.join(".claude/settings.json")];
    let mut profiles: Vec<PathBuf> = std::fs::read_dir(state.join("north/profiles"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with("stale-"))
        .map(|entry| entry.path().join("settings.json"))
        .collect();
    profiles.sort();
    files.extend(profiles);
    files.retain(|file| file.is_file());
    files
}

pub(crate) fn paths(settings: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    let mut stack = vec![settings];
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(map) => {
                if let Some(command) = map.get("command").and_then(Value::as_str) {
                    paths.extend(command.split_whitespace().map(|word| word.trim_matches(['"', '\''])).filter(|word| word.starts_with('/') && !word.contains('=')).map(str::to_owned));
                }
                stack.extend(map.values().rev());
            }
            Value::Array(items) => stack.extend(items.iter().rev()),
            _ => {}
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::paths;
    use serde_json::json;

    #[test]
    fn a_hook_command_names_its_programs_and_scripts_but_not_its_environment_values() {
        let settings = json!({"hooks": {"PreToolUse": [{"hooks": [{"type": "command", "command": "/etc/codex/hooks/runtime/env -u BASH_ENV PATH=/a:/b /etc/codex/hooks/runtime/bash '/home/tom/.agents/hooks/guard.sh'"}]}]}});
        assert_eq!(paths(&settings), ["/etc/codex/hooks/runtime/env", "/etc/codex/hooks/runtime/bash", "/home/tom/.agents/hooks/guard.sh"]);
    }
}
