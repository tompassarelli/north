//! The loads the harness itself requires of a proxy, which spend none of its read budget.
use crate::guard::paths::{basename, expanduser, join, normpath};
use north_base::shell::{is_separator, words};
use serde_json::Value;

const CONFIG_ENV: &str = "CLAUDE_CONFIG_DIR";

pub fn spends_no_look(tool: &str, input: &Value) -> bool {
    let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let text = |key: &str| input[key].as_str().unwrap_or_default();
    let (file, command) = (text("file_path"), text("command"));
    var("HOME").is_some_and(|home| harness_load(&home, var(CONFIG_ENV).as_deref(), tool, if tool == "Read" { file } else { command }))
}

pub fn harness_load(home: &str, config: Option<&str>, tool: &str, text: &str) -> bool {
    match tool {
        "Skill" => true,
        "Read" => {
            let path = normpath(&join(home, &expanduser(text, home)));
            let profiles = format!("{home}/.local/state/north/profiles/");
            config.is_some_and(|config| path.starts_with(&format!("{}/skills/", config.trim_end_matches('/')))) || path.starts_with(&format!("{home}/.claude/skills/")) || (path.starts_with(&profiles) && path.contains("/skills/"))
        }
        "Bash" => {
            let Some(words) = words(text) else { return false };
            let words: Vec<&str> = words.iter().map(String::as_str).collect();
            !words.iter().any(|word| is_separator(word)) && matches!(words.as_slice(), [north, rest @ ..] if basename(north) == "north" && (rest.contains(&"--help") || matches!(rest, ["work", "brief", "new", ..])))
        }
        _ => false,
    }
}
