use std::process::Command;

use super::{Part, joins, script, split};

fn ran(checks: &[(&str, u64)]) -> String {
    let output = Command::new("bash").args(["-euo", "pipefail", "-c", &script(checks)]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn checks_run_side_by_side_and_each_keeps_its_own_exit_output_and_deadline() {
    let log = ran(&[("sleep 1; echo first", 30), ("echo 'it''s quoted' \"$HOME\" >&2; exit 3", 30), ("sleep 1; echo third", 30), ("sleep 30", 1)]);
    let parts = split(&log, 4);
    let [Some(first), Some(second), Some(third), Some(late)] = parts.as_slice() else { panic!("{log}") };
    assert_eq!((first.status, first.text.as_str()), (0, "first\n"));
    assert_eq!((second.status, second.text.as_str()), (3, format!("its quoted {}\n", std::env::var("HOME").unwrap()).as_str()));
    assert_eq!((third.status, third.text.as_str(), late.status), (0, "third\n", 124));
    assert!(first.seconds >= 0.9 && third.seconds >= 0.9 && third.offset < 0.5, "the checks ran one after another: {parts:?}");
    assert_eq!(split("offload: running abc in slot\nland batch: check 0 exit 0 from 10.5 to 12.0\nok\n", 2), vec![Some(Part { status: 0, offset: 0.0, seconds: 1.5, text: "ok\n".to_owned() }), None]);
    assert!(joins("CARGO_TARGET_DIR=target ./bin/head-north --code src/cli/ citation-check --root $PWD"));
    assert!(!joins("CARGO_TARGET_DIR=target ./bin/head-north land-tests --select changed --only portable"));
    assert!(!joins("CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo clippy --all-targets"));
    assert!(!joins("CARGO_TARGET_DIR=target ./bin/head-north --code src/cli/ --no-build citation-check --root $PWD"));
}
