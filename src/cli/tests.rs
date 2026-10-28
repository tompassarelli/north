use super::*;

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
