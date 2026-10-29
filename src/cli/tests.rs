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
                    Some(Command::SessionHost(rest.to_vec()))
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
fn every_north_subcommand_cited_in_docs_skills_and_hooks_is_defined() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let undefined = crate::ops::citation_check::undefined(root, defines).unwrap();
    assert!(undefined.is_empty(), "{undefined:?}");
}

#[test]
fn the_cli_defines_what_it_dispatches_and_nothing_else() {
    assert!(["fleet", "config", "guard", "private-desktop", "land-clean"].into_iter().all(defines));
    assert!(!["pace-refusal", "capture", "delegate", ""].into_iter().any(defines));
}
