use super::refused_tool;

#[test]
fn a_bare_pi_run_is_refused_in_command_position_and_its_maintenance_words_are_not() {
    for command in ["pi --mode json hi", "cd lane && pi hi", "env FOO=1 /usr/bin/pi hi", "bash -c 'pi hi'"] {
        assert_eq!(refused_tool(command).as_deref(), Some("pi"), "{command}");
    }
    for command in ["pi --version", "pi --help", "rg pi docs", "echo \"pi run\"", "north fleet launch --provider pi --name w", "python3 -c 'print(3.14)'"] {
        assert_eq!(refused_tool(command), None, "{command}");
    }
}
