use super::*;
use super::tools::defines;

#[test]
fn command_grammar_preserves_payloads_and_rejects_unknown_prefixes() {
    let alphabet = [
        "config",
        "agents",
        "work",
        "session-host",
        "dep-gate",
        "contract-probe",
        "-h",
        "--help",
        "help",
        "",
        "--unknown",
        "bridge",
        "Ångström",
    ];
    for length in 0..=4 {
        for mut number in 0..alphabet.len().pow(length) {
            let mut args = Vec::new();
            for _ in 0..length {
                args.push(alphabet[number % alphabet.len()].to_owned());
                number /= alphabet.len();
            }
            let expected = match args.as_slice() {
                [] => Some(Command::Fleet(Vec::new())),
                [help] if ["help", "-h", "--help"].contains(&help.as_str()) => Some(Command::Help),
                [config, agents, rest @ ..] if config == "config" && agents == "agents" => {
                    Some(Command::Agents(rest.to_vec()))
                }
                [agents, rest @ ..] if agents == "agents" => {
                    Some(Command::Tool(Tool::Uniform("agents"), rest.to_vec()))
                }
                [work, rest @ ..] if work == "work" => Some(Command::Work(rest.to_vec())),
                [host, rest @ ..] if host == "session-host" => {
                    Some(Command::Session(Session::Host, rest.to_vec()))
                }
                [gate, rest @ ..] if gate == "dep-gate" => {
                    Some(Command::Tool(Tool::Uniform("dep-gate"), rest.to_vec()))
                }
                [probe, rest @ ..] if probe == "contract-probe" => {
                    Some(Command::ContractProbe(rest.to_vec()))
                }
                _ => None,
            };
            match expected {
                Some(expected) => {
                    assert_eq!(parse_command(args.clone()).unwrap(), expected, "{args:?}")
                }
                None => assert!(
                    matches!(parse_command(args.clone()), Err(NorthError::Usage(_))),
                    "{args:?}"
                ),
            }
        }
    }
}

#[test]
fn every_citation_in_docs_skills_and_hooks_exists_and_every_doc_is_linked() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let undefined = crate::ops::citation_check::undefined(root, defines).unwrap();
    assert!(undefined.is_empty(), "{undefined:?}");
    let flags = crate::ops::citation_check::missing_flags(root, defines).unwrap();
    assert!(flags.is_empty(), "{flags:?}");
    let keys = crate::ops::citation_check::missing_keys(root).unwrap();
    assert!(keys.is_empty(), "{keys:?}");
    let broken = crate::ops::citation_check::broken_paths(root).unwrap();
    assert!(broken.is_empty(), "{broken:?}");
    let anchors = crate::ops::citation_check::broken_anchors(root).unwrap();
    assert!(anchors.is_empty(), "{anchors:?}");
    let unlinked = crate::ops::citation_check::unlinked_docs(root).unwrap();
    assert!(unlinked.is_empty(), "{unlinked:?}");
}

#[test]
fn a_review_dispatch_of_the_proxy_role_plan_prints_items_by_kind_with_fact_results_and_tiers() {
    use crate::ops::review_items::{dispatch, render, Tier};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let plan = "docs/rationale/proxy-role.md";
    let lines = render(plan, Tier::OpusMedium, &dispatch(root, plan, Tier::OpusMedium, defines).unwrap());
    assert!(lines[0].starts_with("review items docs/rationale/proxy-role.md, author opus-medium: "), "{}", lines[0]);
    assert!(lines[0].contains("scenario -> sonnet-high") && lines[0].contains("architecture -> opus-high"), "{}", lines[0]);
    let first = |prefix: &str| lines.iter().position(|line| line.starts_with(prefix)).unwrap_or_else(|| panic!("no {prefix} line in {lines:#?}"));
    assert!(first("fact ") < first("scenario ") && first("scenario ") < first("architecture "), "{lines:#?}");
    assert!(lines.iter().any(|line| line.starts_with("fact holds ") && line.contains("path src/cli/fleet/lineage.rs:19: line 19 of ")), "{lines:#?}");
    assert!(lines.iter().any(|line| line.starts_with("fact holds ") && line.ends_with("verb north fleet: defined")), "{lines:#?}");
    assert!(lines.iter().any(|line| line.starts_with("fact holds ") && line.ends_with("boxes R1 and R3 both change src/cli/fleet/launch.rs, src/cli/fleet/lineage.rs; R3 needs R1")), "{lines:#?}");
    assert!(lines.iter().any(|line| line.starts_with("fact FAILS ") && line.contains("boxes W1 and L2 both change crates/north-landing/src/land/after/mod.rs and no Needs: orders them")), "{lines:#?}");
    assert!(lines.iter().any(|line| line.starts_with("scenario sonnet-high ") && line.contains(": 2.1 The sender is a required value: Enforced by: types.")), "{lines:#?}");
    assert!(lines.iter().any(|line| line.starts_with("architecture opus-high lens simpler-alternative ") && line.ends_with(": 2.3 The session Tom types into is that fleet session")), "{lines:#?}");
    assert!(lines.iter().filter(|line| line.starts_with("scenario ")).all(|line| line.starts_with("scenario sonnet-high ")), "{lines:#?}");
    assert!(lines.iter().filter(|line| line.starts_with("architecture ")).all(|line| line.starts_with("architecture opus-high lens ")), "{lines:#?}");
}

#[test]
fn the_cli_defines_what_it_dispatches_and_nothing_else() {
    assert!(["fleet", "config", "guard", "private-desktop", "land-clean"].into_iter().all(defines));
    assert!(!["pace-refusal", "capture", "delegate", ""].into_iter().any(defines));
}
