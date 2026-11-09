use super::refused_tool;

#[test]
fn a_bare_opencode_run_is_refused_in_command_position_and_its_maintenance_words_are_not() {
    for command in ["opencode run -m opencode-go/deepseek-v4.1-flash hi", "cd lane && opencode run hi", "env FOO=1 /run/current-system/sw/bin/opencode run hi", "bash -c 'opencode run hi'"] {
        assert_eq!(refused_tool(command).as_deref(), Some("opencode"), "{command}");
    }
    for command in ["opencode --version", "opencode models", "rg opencode docs", "echo \"opencode run\"", "north fleet launch --provider opencode --name w"] {
        assert_eq!(refused_tool(command), None, "{command}");
    }
}
