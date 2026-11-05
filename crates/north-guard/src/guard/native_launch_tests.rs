use super::native_launch::launches_unwrapped_client;
use super::trigger_matrix::exercise;

const SESSION: &str = "bun /opt/machine-capacity.mjs session --class native --owner x --";

fn wine_proton_and_exe_launches_are_refused_in_command_position() {
    exercise(
        launches_unwrapped_client,
        &[
            "wine game.exe",
            "wine64 game.exe",
            "proton run game.exe",
            "steam-run wine game.exe",
        ],
        &[
            "ls game.exe",
            "pgrep -f game.exe",
            &format!("{SESSION} wine game.exe"),
        ],
        true,
    );
}

fn a_native_machine_capacity_session_wraps_a_client() {
    for command in [
        format!("{SESSION} wine game.exe"),
        "machine-capacity session --class=native --new-clients 1 -- umu-run game".to_owned(),
        "bun x/machine-capacity.mjs session --class native -- ./War3.exe".to_owned(),
    ] {
        assert!(!launches_unwrapped_client(&command), "{command}");
    }
    for command in [
        "bun x/machine-capacity.mjs session --class heavy -- wine game.exe",
        "bun x/machine-capacity.mjs run --class native -- wine game.exe",
        "env FOO=1 nohup ./War3.EXE --windowed",
        "echo ready; gamemoderun umu-run game",
        "sudo -u tom wine64 setup.exe",
    ] {
        assert!(launches_unwrapped_client(command), "{command}");
    }
}

fn unlexable_commands_pass() {
    assert!(!launches_unwrapped_client("wine 'unterminated"));
}

#[test]
fn native_launch_decisions_hold() {
    wine_proton_and_exe_launches_are_refused_in_command_position();
    a_native_machine_capacity_session_wraps_a_client();
    unlexable_commands_pass();
}
