use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use super::*;
use super::activation::{catalog_registration, restrict_skill_agents};
use super::files::write_json;
use super::generation::{
    instruction_targets, publish_generation, publish_skill_links_in, safe_relative_path, start_clean_generation,
    write_active_list,
};
use super::paths::repo_root_in;
use super::reviews::skill_metadata;

#[test]
fn missing_owner_path_names_its_registration() {
    let error = registration_checks::require_registered_owner(
        false,
        "north",
        "harness/hooks/tripwire-guard.sh",
        "agents/catalog-config.json",
        "registration lane",
    )
    .expect_err("missing owner must refuse");

    assert!(error.to_string().contains("harness/hooks/tripwire-guard.sh"));
    assert!(error.to_string().contains("agents/catalog-config.json"));
}

#[test]
fn registered_hook_paths_for_both_providers_are_refused() {
    for (claude_json, codex_toml, registration_file) in [
        (
            json!({"hooks":{"PreToolUse":[{"hooks":[{
                "type":"command",
                "command":"/home/tom/.agents/hooks/missing-hook.sh"
            }]}]}})
            .to_string(),
            "[hooks]\n".to_owned(),
            "claude.json",
        ),
        (
            "{}".to_owned(),
            "[[hooks.PreToolUse]]\nmatcher = \".*\"\n[[hooks.PreToolUse.hooks]]\ntype = \"command\"\ncommand = \"/home/tom/.agents/hooks/missing-hook.sh\"\n".to_owned(),
            "codex.toml",
        ),
    ] {
        let state = tempfile::tempdir().expect("hook check state");
        let claude = state.path().join("claude.json");
        fs::write(&claude, claude_json).expect("Claude registration");
        let codex = state.path().join("codex.toml");
        fs::write(&codex, codex_toml).expect("Codex registration");
        let provider_hooks = state.path().join("provider-hooks");
        fs::create_dir(&provider_hooks).expect("provider hooks");
        let inputs = registration_checks::HookPathInputs {
            claude,
            codex,
            provider_hooks,
        };
        let error =
            registration_checks::validate_hook_registrations(&inputs).expect_err("missing hook must refuse");

        assert!(error.to_string().contains("missing-hook.sh"));
        assert!(error.to_string().contains(registration_file));
    }
}

#[test]
fn landing_catalog_without_a_registered_hook_refuses_the_nixos_registration() {
    let catalog = json!({
        "activation": {
            "kept-guard": {"distributions": [{"type": "hook", "targets": ["claude"]}]},
            "firn-system-policy": {"distributions": [{"type": "providerAdapter", "adapterId": "firn-system-policy", "targets": ["claude"]}]}
        },
        "registrations": {
            "kept-guard": {"kind": "hook", "owner": {"repo": "north", "path": "harness/hooks/kept-guard.sh"}}
        },
        "providerSupport": [{"owner": {"repo": "north", "path": "harness/defaults.toml"}, "path": "defaults.toml"}]
    });
    let provision = registration_checks::HookProvision::Names(registration_checks::landing_hook_names(&catalog));
    let registered = |name: &str| {
        json!({"hooks": {"PreToolUse": [{"hooks": [{
            "type": "command",
            "command": format!("/home/tom/.agents/hooks/{name}")
        }]}]}})
    };
    let check = |name: &str| {
        registration_checks::validate_hook_commands(&registered(name), "Claude", Path::new("claude-hooks.json"), &provision)
    };

    assert!(check("kept-guard.sh").is_ok());
    assert!(check("firn-system-policy").is_ok());
    assert!(check("defaults.toml").is_ok());
    let error = check("worker-handoff.sh").expect_err("a hook the catalog no longer projects must refuse");
    assert!(error.to_string().contains("worker-handoff.sh"));
    assert!(error.to_string().contains("claude-hooks.json"));
}

#[test]
fn activation_with_a_missing_registered_hook_keeps_the_current_generation() {
    let state = tempfile::tempdir().expect("activation state");
    let current = state.path().join("current");
    fs::create_dir(state.path().join("gen-current")).expect("current generation");
    symlink("gen-current", &current).expect("current link");
    let activation = json!({
        "generationId": "sha256:missing-hook",
        "providerSupport": [{
            "owner": {
                "repo": format!("registration-guard-fixture-{}", std::process::id()),
                "path": "missing-hook.sh"
            },
            "path": "missing-hook.sh"
        }],
        "units": [],
        "baselines": []
    });

    let result = publish_generation(&activation, state.path());

    assert!(result.is_err());
    assert_eq!(
        fs::read_link(current).expect("current remains"),
        Path::new("gen-current")
    );
}

#[test]
fn operator_dir_prefers_configured_then_south_from_repo_roots() {
    let base = env::temp_dir().join(format!("north-operator-dir-{}", std::process::id()));
    let south = base.join("south");
    let roots = json!({"south": south}).to_string();
    let resolve = |configured| operator_dir(configured, |repo| repo_root_in(repo, Some(&roots)));

    assert!(resolve(None).is_err());
    fs::create_dir_all(south.join("agents")).unwrap();
    assert_eq!(resolve(None).unwrap(), south.join("agents"));
    assert_eq!(
        resolve(Some("/elsewhere".into())).unwrap(),
        PathBuf::from("/elsewhere")
    );
    fs::remove_dir_all(&base).unwrap();
}

#[test]
fn checkout_filter_agrees_with_generated_registration_sets() {
    let kinds = [
        "skill",
        "hook",
        "instruction",
        "providerSupport",
        "baseline",
        "module",
    ];
    for mask in 0..64usize {
        for kind in kinds {
            let mut registrations = Map::new();
            let mut retained = Vec::new();
            let mut absent = Vec::new();
            for index in 0..6 {
                let id = format!("unit-{index}");
                let repo = format!("repo-{index}");
                registrations.insert(
                    id.clone(),
                    json!({"kind":kind,"owner":{"repo":repo,"path":"x"}}),
                );
                if mask & (1 << index) != 0 {
                    retained.push(id);
                } else {
                    absent.push(id);
                }
            }
            let result = skip_skills_without_checkout(&mut registrations, |repo| {
                let index: usize = repo.strip_prefix("repo-").unwrap().parse().unwrap();
                Ok(mask & (1 << index) != 0)
            });
            if kind != "skill" && !absent.is_empty() {
                assert!(result.is_err());
            } else {
                let skipped = result.unwrap();
                assert_eq!(skipped.keys().cloned().collect::<Vec<_>>(), absent);
                assert_eq!(registrations.keys().cloned().collect::<Vec<_>>(), retained);
            }
        }
    }
}

#[test]
fn generated_package_sources_preserve_repository_and_catalog_base() {
    let mut inputs = crate::test_inputs::Generator::new(0x534f55524345);
    for case in 0..crate::test_inputs::cases() {
        let repo = inputs.word();
        let directories = (0..case % 6).map(|_| inputs.word()).collect::<Vec<_>>();
        let source = (0..1 + inputs.index(6)).map(|_| inputs.word()).collect::<Vec<_>>();
        let catalog = directories.iter().map(String::as_str).chain(std::iter::once("catalog.json")).collect::<Vec<_>>().join("/");
        let expected = directories.iter().chain(&source).map(String::as_str).collect::<Vec<_>>().join("/");
        let registration = catalog_registration(
            &json!({"id": inputs.word(), "kind": "skill", "source": source.join("/")}),
            &json!({"repo": repo, "path": catalog}),
        ).expect("generated relative source must resolve");
        assert_eq!(registration.get("owner"), Some(&json!({"repo": repo, "path": expected})), "case {case}");
    }
}

#[test]
fn activation_paths_match_generated_chain_permissions() {
    for depth in 1..=8usize {
        for mask in 0..(1usize << depth) {
            let ids = (0..depth).map(|i| format!("unit-{i}")).collect::<Vec<_>>();
            let mut permissions = Map::new();
            let mut inputs = Vec::new();
            for (index, id) in ids.iter().enumerate() {
                permissions.insert(
                    id.clone(),
                    json!(if mask & (1 << index) != 0 {
                        "on"
                    } else {
                        "off"
                    }),
                );
                let members = if index + 1 < depth && index % 2 == 0 {
                    vec![ids[index + 1].clone()]
                } else {
                    vec![]
                };
                let supports = if index > 0 && index % 2 == 0 {
                    vec![ids[index - 1].clone()]
                } else {
                    vec![]
                };
                inputs.push(json!({"id":id,"members":members,"supports":supports}));
            }
            let mut activation =
                json!({"rootOrder":[ids[0]],"permissions":permissions,"units":inputs});
            recompute_activation(&mut activation).unwrap();
            for (index, id) in ids.iter().enumerate() {
                let reachable = (0..=index).all(|i| mask & (1 << i) != 0);
                let unit = find_unit(&activation, id).unwrap();
                assert_eq!(unit["active"], json!(reachable));
                let expected = if reachable {
                    vec![ids[..=index].to_vec()]
                } else {
                    vec![]
                };
                assert_eq!(unit["activationPaths"], json!(expected));
            }
        }
    }
}

#[test]
fn active_list_names_exactly_the_units_marked_active() {
    let mut activation = json!({
        "rootOrder": ["root"],
        "permissions": {"root": "on", "skill": "on", "guard": "off", "idle": "on"},
        "units": [
            {"id": "root", "kind": "module", "members": ["skill"], "supports": []},
            {"id": "skill", "kind": "skill", "members": [], "supports": []},
            {"id": "guard", "kind": "hook", "members": [], "supports": ["skill"]},
            {"id": "idle", "kind": "skill", "members": [], "supports": []}
        ]
    });
    recompute_activation(&mut activation).expect("activation must resolve");
    let directory = env::temp_dir().join(format!("north-active-list-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("fixture must exist");
    write_active_list(&directory, &activation).expect("active list must write");
    let list = fs::read_to_string(directory.join("activation.active"))
        .expect("active list must read");
    fs::remove_dir_all(&directory).expect("fixture must clean up");
    // Hook activation readers consume one kind/id record per line.
    assert_eq!(list, "module root\nskill skill\n");
}

#[test]
fn generated_permissions_follow_registration_set_changes() {
    let mut inputs = crate::test_inputs::Generator::new(0x5045524d4954);
    for case in 0..crate::test_inputs::cases() {
        let count = 3 + inputs.index(30);
        let mut previous = Map::new();
        let mut registered = Vec::new();
        let mut expected = Map::new();
        for at in 0..count {
            let id = format!("{at}-{}", inputs.word());
            let permission = ["on", "off"][inputs.index(2)];
            match at % 3 {
                0 => {
                    previous.insert(id.clone(), json!(permission));
                    registered.push(json!({"id": id}));
                    expected.insert(id, json!(permission));
                },
                1 => { previous.insert(id, json!(permission)); },
                _ => {
                    registered.push(json!({"id": id}));
                    expected.insert(id, json!("off"));
                },
            }
        }
        let mut activation = json!({"permissions": previous, "units": registered});
        ensure_permissions(&mut activation).expect("generated permissions must refresh");
        assert_eq!(activation["permissions"], Value::Object(expected), "case {case}");
        let refreshed = activation.clone();
        ensure_permissions(&mut activation).expect("refresh must be idempotent");
        assert_eq!(activation, refreshed, "case {case}");
    }
}

#[test]
fn codex_links_retire_ids_that_left_the_generation() {
    let fixture =
        env::temp_dir().join(format!("north-codex-link-prune-{}", std::process::id()));
    let directory = fixture.join("codex-skills");
    let root = fixture.join("agents");
    let generation = root.join("gen-new");
    let old_generation = root.join("gen-old");
    let foreign = fixture.join("foreign/skill");
    for path in [&directory, &generation, &old_generation, &foreign] {
        fs::create_dir_all(path).expect("fixture directory must exist");
    }
    fs::create_dir_all(directory.join(".system")).expect("provider directory must exist");
    symlink(
        old_generation.join("skills/shared/renamed-old"),
        directory.join("renamed-old"),
    )
    .expect("stale generation link must exist");
    symlink(&foreign, directory.join("manifest-only")).expect("manifest link must exist");
    symlink(&foreign, directory.join("user-owned")).expect("user link must exist");
    write_json(
        &root.join("codex-managed-skills.json"),
        &json!({"schema": MANAGED_SKILLS_SCHEMA, "ids": ["manifest-only"]}),
    )
    .expect("previous manifest must write");
    let activation = json!({"units": [
        {"id": "renamed", "active": true, "distributions": [{"type": "skill", "targets": ["shared"]}]},
        {"id": "inactive", "active": false, "distributions": [{"type": "skill", "targets": ["shared"]}]}
    ]});

    publish_skill_links_in(
        &directory,
        "codex-managed-skills.json",
        &root,
        &generation,
        &activation,
        "codex",
    )
    .expect("links must publish");

    let mut names = fs::read_dir(&directory)
        .expect("directory must be readable")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .into_string()
                .expect("utf-8")
        })
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, [".system", "renamed", "user-owned"]);
    assert_eq!(
        fs::read_link(directory.join("renamed")).expect("link"),
        generation.join("skills/shared/renamed")
    );
    assert_eq!(
        read_json(&root.join("codex-managed-skills.json")).expect("manifest")["ids"],
        json!(["renamed"])
    );
    fs::remove_dir_all(fixture).expect("fixture must clean up");
}

#[test]
fn a_new_generation_does_not_inherit_vanished_projections() {
    let generation =
        env::temp_dir().join(format!("north-clean-generation-{}", std::process::id()));
    let stale = generation.join("projects/retired/hook/code-upstream-guard");
    fs::create_dir_all(&stale).expect("stale fixture must exist");
    fs::write(
        generation.join("permissions-from-previous-generation"),
        "stale",
    )
    .expect("stale fixture must write");

    start_clean_generation(&generation).expect("generation must reset");

    assert!(generation.is_dir());
    assert!(
        fs::read_dir(&generation)
            .expect("generation must be readable")
            .next()
            .is_none()
    );
    fs::remove_dir_all(generation).expect("fixture must clean up");
}

#[test]
fn claude_only_skill_is_removed_from_codex_and_shared_targets() {
    // Oracle: firn#7 asks for Claude-only delivery and removal from other agents.
    let fixture = env::temp_dir().join(format!("north-skill-agents-{}", std::process::id()));
    let root = fixture.join("agents");
    let generation = root.join("gen-new");
    fs::create_dir_all(&generation).expect("generation");
    let source = fixture.join("SKILL.md");
    fs::write(
        &source,
        "---\nname: example\nagents: [claude]\ndescription: Example.\n---\n",
    )
    .expect("skill");
    let metadata = skill_metadata(&source).expect("metadata");
    let mut distribution = json!({"type": "skill", "targets": ["shared"]});
    restrict_skill_agents(&mut distribution, &metadata).expect("restriction");
    assert_eq!(distribution["targets"], json!(["claude"]));
    let activation =
        json!({"units": [{"id": "example", "active": true, "distributions": [distribution]}]});
    for agent in ["claude", "codex"] {
        let directory = fixture.join(agent);
        fs::create_dir_all(&directory).expect("directory");
        symlink(
            root.join("gen-old/skills/shared/example"),
            directory.join("example"),
        )
        .expect("old link");
        publish_skill_links_in(
            &directory,
            &format!("{agent}-managed-skills.json"),
            &root,
            &generation,
            &activation,
            agent,
        )
        .expect("publish");
        assert_eq!(directory.join("example").is_symlink(), agent == "claude");
    }
    let mut unrestricted = json!({"type": "skill", "targets": ["shared"]});
    restrict_skill_agents(&mut unrestricted, &BTreeMap::new()).expect("unrestricted");
    assert_eq!(unrestricted["targets"], json!(["shared"]));
    fs::remove_dir_all(fixture).expect("fixture cleanup");
}

#[test]
fn generated_skill_frontmatter_preserves_scalar_and_block_values() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("SKILL.md");
    for seed in 0..64 {
        let name = format!("skill-{seed}");
        let parts = (0..seed % 7 + 1).map(|index| format!("世界 {seed} part {index}")).collect::<Vec<_>>();
        let style = seed % 7;
        let description = parts.join(if style >= 5 { "\n" } else { " " });
        let value = match style {
            0 => description.clone(),
            1 => format!("'{description}'"),
            2 => format!("\"{description}\""),
            _ => format!("{}\n  {}", [">", ">-", "|", "|-"][style - 3], parts.join("\n  \n  ")),
        };
        fs::write(&path, format!("---\nname: {name}\ndescription: {value}\n---\nbody: excluded\n")).unwrap();
        assert_eq!(skill_metadata(&path).unwrap(), BTreeMap::from([
            ("name".into(), name), ("description".into(), description),
        ]));
    }
}

#[test]
fn instruction_targets_are_the_union_of_baselines_and_active_instruction_distributions() {
    for mask in 0..256usize {
        let mut inputs = Vec::new();
        let mut expected = BTreeSet::from(["shared".to_owned(), "code".to_owned()]);
        for index in 0..8 {
            let active = mask & (1 << index) != 0;
            let target = format!("target-{index}");
            let kind = if index % 2 == 0 {
                "instructions"
            } else {
                "skill"
            };
            if active && kind == "instructions" {
                expected.insert(target.clone());
            }
            inputs.push(json!({"id":format!("unit-{index}"),"active":active,"distributions":[{"type":kind,"targets":[target,"shared"]}]}));
        }
        let activation = json!({"baselines":[{"targets":["shared","code"]}],"units":inputs});
        assert_eq!(instruction_targets(&activation).unwrap(), expected);
    }
}

#[test]
fn projection_paths_accept_relative_components_and_reject_absolute_or_parent_components() {
    let parts = ["lib", "Ångström", "a skill", ".", ".."];
    assert!(safe_relative_path("").is_err());
    for first in parts {
        for second in parts {
            for third in parts {
                let relative = format!("{first}/{second}/{third}");
                let expected = ![first, second, third].contains(&"..");
                assert_eq!(
                    safe_relative_path(&relative).is_ok(),
                    expected,
                    "{relative}"
                );
                if expected {
                    assert_eq!(
                        safe_relative_path(&relative).unwrap(),
                        PathBuf::from(&relative)
                    );
                }
                assert!(safe_relative_path(&format!("/{relative}")).is_err());
            }
        }
    }
}
