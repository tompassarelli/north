use std::path::Path;
use std::process::Command;

#[test]
fn red_main_notify_messages_each_culprit_lane_and_the_proxy_once_per_failing_commit() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/red-main-notify.check.sh");
    let output = Command::new("bash").arg(&script).output().unwrap();
    assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
