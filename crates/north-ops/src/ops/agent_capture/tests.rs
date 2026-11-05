use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::{Geometry, Programs, geometry, run_with};

fn scene(active: bool, tile: Value) -> (Value, Value, Value) {
    (
        json!([{"id": 7, "workspace_id": 2, "layout": {"tile_pos_in_workspace_view": tile, "window_offset_in_tile": [4.5, 6.0], "window_size": [800, 600]}}]),
        json!([{"id": 2, "output": "DP-1", "is_active": active}]),
        json!({"DP-1": {"logical": {"x": 1920, "y": 100}}}),
    )
}

#[test]
fn a_visible_floating_window_maps_to_its_logical_region() {
    let (windows, workspaces, outputs) = scene(true, json!([10.25, 20.0]));
    assert_eq!(geometry(&windows, &workspaces, &outputs, 7), Geometry::At("1934,126 800x600".to_owned()));
}

#[test]
fn a_window_that_is_absent_or_off_screen_has_no_region() {
    let (windows, workspaces, outputs) = scene(true, json!([10.0, 20.0]));
    assert_eq!(geometry(&windows, &workspaces, &outputs, 8), Geometry::Missing);
    let (windows, workspaces, outputs) = scene(false, json!([10.0, 20.0]));
    assert_eq!(geometry(&windows, &workspaces, &outputs, 7), Geometry::Hidden);
    let (windows, workspaces, outputs) = scene(true, Value::Null);
    assert_eq!(geometry(&windows, &workspaces, &outputs, 7), Geometry::Hidden);
}

fn stub(dir: &Path, name: &str, body: &str) -> String {
    let path = dir.join(name);
    crate::foundation::shim::write_script(&path, &format!("{body}\n"));
    path.to_string_lossy().into_owned()
}

fn invoke(arguments: &[&str], programs: &Programs) -> (u8, Vec<String>, Vec<String>) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let arguments: Vec<String> = arguments.iter().map(|word| (*word).to_owned()).collect();
    let code = run_with(&arguments, programs, &mut |line| out.push(line.to_owned()), &mut |line| err.push(line.to_owned()));
    (code, out, err)
}

#[test]
fn each_mode_asks_grim_for_the_right_target_and_prints_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("grim.log");
    let niri = stub(
        dir.path(),
        "niri",
        r#"case "$3" in
focused-output) echo '{"name":"DP-1"}' ;;
windows) echo '[{"id":7,"workspace_id":2,"layout":{"tile_pos_in_workspace_view":[10,20],"window_offset_in_tile":[0,0],"window_size":[640,480]}},{"id":9,"workspace_id":3,"layout":{"tile_pos_in_workspace_view":null,"window_offset_in_tile":[0,0],"window_size":[1,1]}}]' ;;
workspaces) echo '[{"id":2,"output":"DP-1","is_active":true},{"id":3,"output":"DP-1","is_active":false}]' ;;
outputs) echo '{"DP-1":{"logical":{"x":0,"y":0}}}' ;;
esac"#,
    );
    let grim = stub(dir.path(), "grim", &format!("echo \"$@\" >> {}", log.display()));
    let programs = Programs { niri, grim };
    assert_eq!(invoke(&["--screen", "/tmp/a.png"], &programs), (0, vec!["/tmp/a.png".to_owned()], vec![]));
    assert_eq!(invoke(&["--window", "7", "/tmp/b.png"], &programs).0, 0);
    assert_eq!(invoke(&["--region", "1,2 3x4", "/tmp/c.png"], &programs).0, 0);
    assert_eq!(fs::read_to_string(log).unwrap(), "-o DP-1 /tmp/a.png\n-g 10,20 640x480 /tmp/b.png\n-g 1,2 3x4 /tmp/c.png\n");
}

#[test]
fn bad_use_and_unreachable_windows_exit_with_distinct_codes() {
    let dir = tempfile::tempdir().unwrap();
    let niri = stub(dir.path(), "niri", r#"case "$3" in windows) echo '[]' ;; *) echo '[]' ;; esac"#);
    let grim = stub(dir.path(), "grim", "exit 5");
    let programs = Programs { niri, grim };
    assert_eq!(invoke(&[], &programs).0, 2);
    assert_eq!(invoke(&["--bogus"], &programs).0, 2);
    assert_eq!(invoke(&["--window", "x", "/tmp/o.png"], &programs).0, 2);
    assert_eq!(invoke(&["--window", "7", "/tmp/o.png"], &programs).2, ["agent-capture: no niri window with id 7"]);
    assert_eq!(invoke(&["--region", "1,2 3x4", "/tmp/o.png"], &programs).2, ["agent-capture: grim failed"]);
}
