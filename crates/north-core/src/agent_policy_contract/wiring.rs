use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::LazyLock;

use super::ordered_json::Json;

pub(super) const PROVIDERS: [&str; 2] = ["claude", "codex"];
pub(super) const EVENT_ORDER: [&str; 7] = [
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "UserPromptSubmit",
    "Stop",
    "SubagentStop",
    "SessionStart",
];
const HOOK_TIMEOUT: u32 = crate::foundation::hook_deadline::HOOK_TIMEOUT_SECS;
const RUNTIME: &str = "/etc/codex/hooks/runtime";
const SEARCH_PATH: &str = "PATH=/etc/codex/hooks/runtime:/home/tom/.local/share/firn/bin:/home/tom/.local/share/north/bin:/home/tom/.local/share/south/bin:/run/current-system/sw/bin";
const CODEX_HEADER: &str = "allow_managed_hooks_only = true
allow_remote_control = false

[features]
hooks = true

[hooks]
managed_dir = \"/etc/codex/hooks\"
";

static PROVIDER_ADAPTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\(providerAdapter\s+"([^"]+)"\)"#).unwrap());

pub(super) type Wiring = Vec<(String, Vec<(String, Vec<String>)>)>;

pub(super) fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

pub(super) fn bindings<'a>(guard: &'a Value, provider: &str) -> Vec<&'a str> {
    guard
        .get(provider)
        .and_then(Value::as_array)
        .map(|bindings| bindings.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

pub(super) fn guards(policy: &Value) -> &[Value] {
    policy.get("guard").and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn guard_command(guard: &Value) -> String {
    format!("{RUNTIME}/env -u BASH_ENV -u ENV {SEARCH_PATH} north hook {}", text(guard, "north_hook"))
}

pub(super) fn wiring(policy: &Value, provider: &str) -> Wiring {
    let mut events: Wiring = Vec::new();
    for guard in guards(policy) {
        for binding in bindings(guard, provider) {
            let (event, matcher) = binding.split_once(':').unwrap_or((binding, ""));
            let command = guard_command(guard);
            let groups = match events.iter().position(|(known, _)| known == event) {
                Some(index) => &mut events[index].1,
                None => {
                    events.push((event.to_owned(), Vec::new()));
                    &mut events.last_mut().unwrap().1
                }
            };
            match groups.iter_mut().find(|(known, _)| known == matcher) {
                Some((_, commands)) => commands.push(command),
                None => groups.push((matcher.to_owned(), vec![command])),
            }
        }
    }
    events.sort_by_key(|(event, _)| {
        EVENT_ORDER.iter().position(|known| known == event).unwrap_or(EVENT_ORDER.len())
    });
    events
}

fn claude_group(matcher: String, commands: Vec<String>) -> Json {
    let commands = commands
        .into_iter()
        .map(|command| {
            Json::object([
                ("type", Json::String("command".into())),
                ("command", Json::String(command)),
                ("timeout", Json::Number(HOOK_TIMEOUT.into())),
            ])
        })
        .collect();
    let mut group = Vec::new();
    if !matcher.is_empty() {
        group.push(("matcher".to_owned(), Json::String(matcher)));
    }
    group.push(("hooks".to_owned(), Json::Array(commands)));
    Json::Object(group)
}

pub(super) fn render_claude(policy: &Value, settings: Vec<(String, Json)>) -> String {
    let hooks = wiring(policy, "claude")
        .into_iter()
        .map(|(event, groups)| {
            let groups = groups
                .into_iter()
                .map(|(matcher, commands)| claude_group(matcher, commands))
                .collect();
            (event, Json::Array(groups))
        })
        .collect();
    let mut document = settings;
    document.push(("hooks".to_owned(), Json::Object(hooks)));
    Json::Object(document).render() + "\n"
}

pub(super) fn render_codex(policy: &Value) -> String {
    let mut out = String::from(CODEX_HEADER);
    for (event, groups) in wiring(policy, "codex") {
        for (matcher, commands) in groups {
            out.push_str(&format!("\n[[hooks.{event}]]\n"));
            if !matcher.is_empty() {
                out.push_str(&format!("matcher = {}\n", north_base::json_text::ascii_string_escaping_del(&matcher)));
            }
            for command in commands {
                out.push_str(&format!(
                    "\n[[hooks.{event}.hooks]]\ntype = \"command\"\ncommand = {}\ntimeout = {HOOK_TIMEOUT}\n",
                    north_base::json_text::ascii_string_escaping_del(&command)
                ));
            }
        }
    }
    out
}

pub(super) fn codex_adapters(catalog: &Value) -> Vec<String> {
    let support = catalog.get("providerSupport").and_then(Value::as_array);
    let adapters: BTreeSet<String> = support.into_iter().flatten().map(|entry| text(entry, "path").to_owned()).collect();
    adapters.into_iter().collect()
}

pub(super) fn nix_adapters(module: &str) -> Vec<String> {
    let mut adapters: Vec<String> = PROVIDER_ADAPTER
        .captures_iter(module)
        .map(|found| found[1].to_owned())
        .collect();
    adapters.sort();
    adapters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repo_policy_wires_prompt_grounding_as_the_north_hook_verb_the_claude_config_ships() {
        let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../harness/policy-owners.toml");
        let policy = crate::platform::toml_json::parse(&std::fs::read_to_string(manifest).unwrap()).unwrap();
        let commands: Vec<String> = wiring(&policy, "claude")
            .into_iter()
            .filter(|(event, _)| event == "UserPromptSubmit")
            .flat_map(|(_, groups)| groups.into_iter().flat_map(|(_, commands)| commands))
            .collect();
        assert!(commands.iter().any(|command| command.ends_with(" north hook prompt-grounding")), "{commands:?}");
        assert!(!commands.iter().any(|command| command.contains("prompt-grounding.sh")), "{commands:?}");
    }
}
