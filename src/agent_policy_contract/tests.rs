use super::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use tempfile::TempDir;

const POLICY: &str = r#"version = 1
guard = [
  { unit = "language-guard", command = "language-guard.sh", claude = ["PreToolUse:Write|Edit"], codex = ["PreToolUse:^Bash$", "Stop"] },
]
"#;

const HOOK: &str = ". lib/authoring-killswitch.sh\nauthoring_guards_off && exit 0\n";

struct Fixture {
    root: TempDir,
}

impl Fixture {
    fn new(policy: &str) -> Self {
        let fixture = Self { root: tempfile::tempdir().unwrap() };
        for directory in ["modules/codex", "modules/north-profile", "agents", "north/harness/hooks"] {
            fs::create_dir_all(fixture.path(directory)).unwrap();
        }
        fixture.put("agents/AGENTS.md", "# Bootstrap\n\n- Check the owner.\n");
        fixture.put("AGENTS.md", "# Machine\n\n- Use Nix modules.\n");
        fixture.put("policy.toml", policy);
        fixture.put("modules/north-profile/claude-hooks.json", "{\"subagentPromptCacheTtl\": \"1h\"}");
        fixture.put("modules/codex/requirements.toml", "");
        fixture.put(
            "modules/codex/default.nix",
            "[\n  (providerAdapter \"language-guard.sh\")\n  (providerAdapter \"lib/north-agent-activation.sh\")\n]\n",
        );
        fixture.put("north/harness/hooks/language-guard.sh", HOOK);
        fixture.put(
            "agents/catalog-config.json",
            &json!({
                "activation": {"language-guard": {"distributions": [{"type": "hook", "targets": ["claude", "codex"]}]}},
                "registrations": {"language-guard": {"owner": {"path": "harness/hooks/language-guard.sh"}}},
                "providerSupport": [{"path": "lib/north-agent-activation.sh"}],
            })
            .to_string(),
        );
        fixture
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    fn put(&self, relative: &str, body: &str) {
        fs::write(self.path(relative), body).unwrap();
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.path(relative)).unwrap()
    }

    fn run(&self, write: bool, local: bool) -> (Vec<String>, Locations) {
        let options = Options {
            repo: self.root.path().to_path_buf(),
            north: Some(self.path("north")),
            local,
            write,
            codex_adapters: false,
        };
        let environment: BTreeMap<&str, PathBuf> = BTreeMap::from([
            ("AGENTS_OPERATOR_DIR", self.path("agents")),
            ("AGENT_POLICY_MANIFEST", self.path("policy.toml")),
            ("AGENT_POLICY_ACTIVATION", self.path("activation.json")),
        ]);
        let locations = Locations::resolve(&options, &|name| {
            environment.get(name).map(|path| path.to_string_lossy().into_owned())
        });
        let errors = match execute(&options, &locations).unwrap() {
            Outcome::Checked(errors) => errors,
            Outcome::Adapters(_) => unreachable!(),
        };
        (errors, locations)
    }

    fn without_hooks(&self) {
        self.put(
            "agents/catalog-config.json",
            r#"{"activation":{},"registrations":{},"providerSupport":[{"path":"lib/north-agent-activation.sh"}]}"#,
        );
        self.put("modules/north-profile/claude-hooks.json", "{}");
        self.put("modules/codex/default.nix", "[\n  (providerAdapter \"lib/north-agent-activation.sh\")\n]\n");
    }
}

#[test]
fn a_nix_module_missing_a_catalog_adapter_is_refused_after_wiring_is_written() {
    let fixture = Fixture::new("");
    fixture.without_hooks();
    let (errors, _) = fixture.run(true, false);
    assert_eq!(errors, Vec::<String>::new());
    fixture.put("modules/codex/default.nix", "[\n]\n");
    let (errors, _) = fixture.run(false, false);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("modules/codex/default.nix providerAdapter list must be exactly"), "{errors:?}");
}

#[test]
fn written_wiring_passes_the_check_and_keeps_settings_order_and_escapes_non_ascii() {
    let fixture = Fixture::new(POLICY);
    fixture.put(
        "modules/north-profile/claude-hooks.json",
        "{\"zeta\": [], \"alpha\": {\"b\": \"caf\u{e9}\", \"a\": 1.5}, \"hooks\": {}}",
    );
    let (errors, _) = fixture.run(true, false);
    assert_eq!(errors, Vec::<String>::new());
    let claude = fixture.read("modules/north-profile/claude-hooks.json");
    assert!(
        claude.starts_with("{\n  \"zeta\": [],\n  \"alpha\": {\n    \"b\": \"caf\\u00e9\",\n    \"a\": 1.5\n  },\n  \"hooks\": {\n    \"PreToolUse\": ["),
        "{claude}"
    );
    assert!(
        claude.contains("\"matcher\": \"Write|Edit\",\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"/etc/codex/hooks/runtime/env"),
        "{claude}"
    );
    assert!(claude.ends_with("]\n  }\n}\n"), "{claude}");
    let codex = fixture.read("modules/codex/requirements.toml");
    assert!(codex.starts_with("allow_managed_hooks_only = true\n"));
    assert!(codex.contains("\n[[hooks.PreToolUse]]\nmatcher = \"^Bash$\"\n\n[[hooks.PreToolUse.hooks]]\ntype = \"command\"\n"));
    assert!(codex.contains("\n[[hooks.Stop]]\n\n[[hooks.Stop.hooks]]\n"));
    assert!(codex.find("[[hooks.PreToolUse]]").unwrap() < codex.find("[[hooks.Stop]]").unwrap());
}

#[test]
fn a_hand_edited_projection_is_refused_with_the_regenerate_command() {
    let fixture = Fixture::new(POLICY);
    fixture.run(true, false);
    let edited = fixture.read("modules/codex/requirements.toml").replace("timeout = 10", "timeout = 99");
    fixture.put("modules/codex/requirements.toml", &edited);
    let (errors, _) = fixture.run(false, false);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].ends_with("differs from guard[]; run north agent-policy-contract --repo <firn> --write"),
        "{errors:?}"
    );
}

#[test]
fn guard_asymmetries_unregistered_commands_and_ungated_hooks_are_each_named() {
    let fixture = Fixture::new(
        "guard = [\n  { unit = \"language-guard\", command = \"other.sh\", claude = [\"Nowhere\"] },\n  { unit = \"language-guard\", command = \"language-guard.sh\" },\n]\n",
    );
    fixture.put("north/harness/hooks/other.sh", "echo hi\n");
    let (errors, _) = fixture.run(false, false);
    let joined = errors.join("\n");
    for expected in [
        "language-guard: command 'other.sh' is not the hook catalog-config.json registers for it (\u{27}language-guard.sh\u{27})",
        "language-guard: unknown claude event in 'Nowhere'",
        "language-guard: wired for ['claude'] but catalog-config.json targets ['claude', 'codex']",
        "language-guard: give codex_absent a reason exactly when it has no codex binding",
        "language-guard: wired hook must source lib/authoring-killswitch.sh and exit 0 when authoring_guards_off",
        "invalid or duplicate provider guard unit: 'language-guard'",
    ] {
        assert!(joined.contains(expected), "missing {expected:?} in {joined}");
    }
}

#[test]
fn a_registered_hook_with_no_guard_is_reported_unwired() {
    let fixture = Fixture::new("guard = []\n");
    let (errors, _) = fixture.run(false, false);
    let unwired = "language-guard: registered hook targets ['claude', 'codex'] but is unwired";
    assert!(errors.contains(&unwired.to_owned()), "{errors:?}");
}

#[test]
fn a_block_repeated_across_the_bootstrap_and_repo_surfaces_is_refused() {
    let fixture = Fixture::new("");
    fixture.without_hooks();
    fixture.put("AGENTS.md", "# Machine\n\n## Rules\n\n- Check   the\n  owner.\n");
    fixture.put("agents/AGENTS.md", "# Bootstrap\n\n## Start\n\n- Check the owner.\n");
    fixture.run(true, false);
    let (errors, _) = fixture.run(false, false);
    assert_eq!(errors, vec!["repo [Rules] repeats the bootstrap [Start] block: - Check the owner."]);
}

#[test]
fn markdown_blocks_skip_fences_headings_and_indented_code_and_join_bullet_continuations() {
    let blocks = surfaces::markdown_blocks(
        "# Title\n\nintro line\ncontinued\n\n## Rules\n\n```\nfenced\n```\n\n    indented\n\n### Deep\n\n* first\n  more\n- second\n",
    );
    let seen: Vec<(&str, &str)> = blocks.iter().map(|block| (block.section.as_str(), block.text.as_str())).collect();
    assert_eq!(
        seen,
        vec![("preamble", "intro line continued"), ("Rules", "* first more"), ("Rules", "- second")]
    );
}

#[test]
fn activation_generation_faults_are_each_named() {
    let fixture = Fixture::new("guard = [{ unit = \"language-guard\", command = \"language-guard.sh\" }]\n");
    let unit = |id: &str, kind: &str, permission: &str, active: bool| {
        json!({
            "id": id, "kind": kind, "title": "t", "triggerDescription": "d", "permission": permission,
            "active": active, "owner": {"repo": "nixos-config", "path": "../escape"}, "members": [],
            "supports": [], "distributions": [], "activationPaths": [],
        })
    };
    fixture.put(
        "activation.json",
        &json!({
            "schema": "north.agent-activation/v1",
            "catalogDigest": format!("sha256:{}", "a".repeat(64)),
            "generationId": "nope",
            "units": [
                unit("language-guard", "skill", "off", true),
                unit("language-guard", "hook", "on", true),
                {"id": "short"},
                7,
            ],
        })
        .to_string(),
    );
    let (errors, _) = fixture.run(false, true);
    let joined = errors.join("\n");
    for expected in [
        "North activation generationId is invalid",
        "North activation unit language-guard is active despite off permission",
        "North activation duplicates global unit id language-guard",
        "NixOS-owned unit language-guard escapes its repository",
        "NixOS-owned unit language-guard source is absent: ../escape",
        "North activation unit 'short' lacks ['activationPaths',",
        "North activation contains a non-object unit",
    ] {
        assert!(joined.contains(expected), "missing {expected:?} in {joined}");
    }
    assert!(!joined.contains("schema is not"), "{joined}");
}

#[test]
fn a_missing_activation_generation_is_unreadable_and_a_hook_unit_missing_from_it_is_named() {
    let fixture = Fixture::new("guard = [{ unit = \"language-guard\", command = \"language-guard.sh\" }]\n");
    let (errors, locations) = fixture.run(false, true);
    let unreadable = format!("North activation generation is unreadable: {}", locations.activation.display());
    assert!(errors.iter().any(|error| error.starts_with(&unreadable)), "{errors:?}");
    fixture.put(
        "activation.json",
        &json!({
            "schema": "north.agent-activation/v1",
            "catalogDigest": format!("sha256:{}", "b".repeat(64)),
            "generationId": format!("sha256:{}", "c".repeat(64)),
            "units": [],
        })
        .to_string(),
    );
    let (errors, _) = fixture.run(false, true);
    let absent = "provider-bound hook is absent from North activation: language-guard";
    assert!(errors.contains(&absent.to_owned()), "{errors:?}");
}

#[test]
fn arguments_require_a_repo_and_reject_unknown_flags() {
    let arguments = |items: &[&str]| items.iter().map(|item| item.to_string()).collect::<Vec<_>>();
    assert_eq!(parse(&arguments(&["--local"])).err().unwrap(), "--repo is required");
    assert_eq!(parse(&arguments(&["--repo"])).err().unwrap(), "--repo needs a value");
    assert_eq!(parse(&arguments(&["--repo", "x", "--bogus"])).err().unwrap(), "unknown argument --bogus");
    assert!(parse(&arguments(&["--repo", "x", "--write", "--local", "--codex-adapters"])).is_ok());
}
