use super::clipboard::touches_clipboard;
use super::trigger_matrix::exercise;

#[test]
fn clipboard_and_screenshot_access_is_refused_in_command_position_only() {
    exercise(
        touches_clipboard,
        &[
            "wl-copy hi",
            "wl-copy --clear",
            "/run/current-system/sw/bin/wl-copy hi",
            "niri msg action screenshot-window",
            "niri msg -j action screenshot-screen",
            "wl-paste",
            "wl-paste --list-types",
            "xclip -o -selection clipboard",
            "xsel -o",
        ],
        &[
            "echo \"wl-paste\"",
            "rg wl-paste socrates",
            "agent-capture --screen /tmp/s.png",
            "niri msg action focus-window --id 3",
        ],
        false,
    );
}
