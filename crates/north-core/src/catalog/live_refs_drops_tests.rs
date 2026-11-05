use serde_json::json;

use super::super::live_refs::{Facts, HookFact, SCHEMA, judge};
use super::*;

fn catalog(hooks: &[&str]) -> Value {
    let mut activation = serde_json::Map::new();
    let mut registrations = serde_json::Map::new();
    for hook in hooks {
        let id = format!("hook-{hook}");
        activation.insert(id.clone(), json!({"distributions": [{"type": "hook"}]}));
        registrations.insert(id, json!({"owner": {"repo": "north", "path": format!("harness/hooks/{hook}")}}));
    }
    json!({"activation": activation, "registrations": registrations, "providerSupport": []})
}

fn host_registering(host: &str, hook: &str) -> Facts {
    Facts {
        schema: SCHEMA.into(),
        host: host.into(),
        recorded_at_ms: 1_000,
        hooks: vec![HookFact { path: format!("/home/tom/.agents/hooks/{hook}"), registered_in: "/home/tom/.claude/settings.json".into() }],
        unit_exec_starts: Vec::new(),
    }
}

#[test]
fn replaying_ea1e659_the_dropped_prompt_grounding_distribution_is_named_and_the_registering_host_refuses_it() {
    let base = catalog(&["kept.sh", "prompt-grounding.sh"]);
    let candidate = catalog(&["kept.sh"]);
    let dropped = dropped_hooks(&base, &candidate, "");
    assert_eq!(dropped.iter().map(String::as_str).collect::<Vec<_>>(), ["prompt-grounding.sh"]);
    let facts = [host_registering("whiterabbit", "prompt-grounding.sh"), host_registering("gex45", "kept.sh")];
    let reports: Vec<_> = dropped.iter().map(|name| (name.clone(), judge(&facts, name, &[], 2_000, 3_600_000))).collect();
    let message = refusal(&reports).expect("refusal");
    assert!(message.contains("prompt-grounding.sh") && message.contains("whiterabbit: /home/tom/.agents/hooks/prompt-grounding.sh registered in"), "{message}");
}

#[test]
fn a_drop_no_host_registers_passes_and_one_with_missing_hosts_refuses_as_unknown() {
    let dropped = dropped_hooks(&catalog(&["gone.sh"]), &catalog(&[]), "");
    let facts = [host_registering("whiterabbit", "kept.sh")];
    let clean: Vec<_> = dropped.iter().map(|name| (name.clone(), judge(&facts, name, &["whiterabbit".into()], 2_000, 3_600_000))).collect();
    assert_eq!(refusal(&clean), None);
    let unknown: Vec<_> = dropped.iter().map(|name| (name.clone(), judge(&facts, name, &["whiterabbit".into(), "cx53".into()], 2_000, 3_600_000))).collect();
    assert!(refusal(&unknown).expect("refusal").contains("cx53 (no facts recorded)"));
}

#[test]
fn a_catalog_that_keeps_or_retires_the_hook_drops_nothing() {
    let base = catalog(&["kept.sh", "prompt-grounding.sh"]);
    assert!(dropped_hooks(&base, &base, "").is_empty());
    assert!(dropped_hooks(&base, &catalog(&["kept.sh"]), "prompt-grounding.sh\n").is_empty());
}
