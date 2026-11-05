use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, Output};

const FAKE_GRIM: &str = "#!/bin/sh
[ \"$XDG_RUNTIME_DIR\" = \"$EXPECTED_RUN/runtime\" ] && [ \"$WAYLAND_DISPLAY\" = wayland-test ] && [ -z \"${WAYLAND_SOCKET+set}\" ] || exit 2
[ \"$1\" = -t ] && [ \"$2\" = png ] || exit 3
case \"$CAPTURE_CASE\" in
    success) printf 'fresh fixture pixels\\n' > \"$3\" ;;
    partial) printf partial > \"$3\"; exit 1 ;;
    empty) : ;;
esac
";

fn capture(run: &Path, case: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["private-desktop", "capture"])
        .arg(run)
        .arg(run.join("frame.png"))
        .env("EXPECTED_RUN", run.canonicalize().unwrap())
        .env("CAPTURE_CASE", case)
        .env("WAYLAND_SOCKET", "999")
        .env("XDG_RUNTIME_DIR", "/invalid")
        .env("WAYLAND_DISPLAY", "wrong")
        .output()
        .unwrap()
}

#[test]
fn capture_runs_grim_in_the_private_environment_and_replaces_the_frame_only_on_a_complete_capture() {
    let fixture = tempfile::tempdir().unwrap();
    let run = fixture.path();
    fs::create_dir(run.join("runtime")).unwrap();
    fs::write(run.join("wayland-display"), "wayland-test\n").unwrap();
    fs::write(run.join("active"), "").unwrap();
    let _server = UnixListener::bind(run.join("runtime/wayland-test")).unwrap();
    let grim = run.join("fake-grim");
    fs::write(&grim, FAKE_GRIM).unwrap();
    fs::set_permissions(&grim, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(run.join("grim"), format!("{}\n", grim.display())).unwrap();
    let frame = run.join("frame.png");

    let output = capture(run, "success");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read_to_string(&frame).unwrap(), "fresh fixture pixels\n");

    for failure in ["partial", "empty"] {
        assert!(!capture(run, failure).status.success(), "{failure} capture succeeded");
        assert_eq!(fs::read_to_string(&frame).unwrap(), "fresh fixture pixels\n", "{failure} replaced the frame");
    }

    fs::remove_file(run.join("active")).unwrap();
    assert!(!capture(run, "success").status.success(), "inactive run captured");
}
