#![cfg(test)]

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
            "north desktop capture --screen /tmp/s.png",
            "niri msg action focus-window --id 3",
        ],
        false,
    );
}

#[test]
fn a_clipboard_aimed_at_a_private_desktop_runtime_is_allowed_but_a_screenshot_action_is_not() {
    let private = "/run/user/1000/private-desktop.qH7nNR/runtime";
    assert!(!touches_clipboard(&format!("XDG_RUNTIME_DIR={private} WAYLAND_DISPLAY=wayland-0 timeout 5 wl-copy --type image/png < a.png")));
    assert!(!touches_clipboard(&format!("export XDG_RUNTIME_DIR={private} WAYLAND_DISPLAY=wayland-0; wl-paste --list-types")));
    assert!(!touches_clipboard("REAL=$(command -v wl-paste); echo $REAL"));
    assert!(touches_clipboard("command wl-paste"));
    assert!(touches_clipboard("XDG_RUNTIME_DIR=/run/user/1000 wl-paste"));
    assert!(touches_clipboard(&format!("export XDG_RUNTIME_DIR={private}; niri msg action screenshot-screen")));
}
