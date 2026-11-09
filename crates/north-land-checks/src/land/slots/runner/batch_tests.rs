use std::process::Command;

use super::{Part, joins, on_one_build, script, split};

fn ran(script: &str) -> String {
    let output = Command::new("bash").args(["-euo", "pipefail", "-c", script]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn checks_run_side_by_side_and_each_keeps_its_own_exit_output_and_deadline() {
    let started = tempfile::tempdir().unwrap();
    let meets = |own: &str, other: &str| format!("touch {0}/{own}; until [ -e {0}/{other} ]; do sleep 0.01; done; echo {own}", started.path().display());
    let log = ran(&script(&[(&meets("first", "third"), 30), ("echo 'it''s quoted' \"$HOME\" >&2; exit 3", 30), (&meets("third", "first"), 30), ("sleep 30", 1)]));
    let parts = split(&log, 4);
    let [Some(first), Some(second), Some(third), Some(late)] = parts.as_slice() else { panic!("{log}") };
    assert_eq!((first.status, first.text.as_str()), (0, "first\n"), "the first check waits for the third to start, so it reached its deadline only if the checks ran one after another: {parts:?}");
    assert_eq!((second.status, second.text.as_str()), (3, format!("its quoted {}\n", std::env::var("HOME").unwrap()).as_str()));
    assert_eq!((third.status, third.text.as_str(), late.status), (0, "third\n", 124));
    assert_eq!(split("offload: running abc in slot\nland batch: check 0 exit 0 from 10.5 to 12.0\nok\n", 2), vec![Some(Part { status: 0, offset: 0.0, seconds: 1.5, text: "ok\n".to_owned() }), None]);
    assert!(joins("CARGO_TARGET_DIR=target ./bin/head-north --code src/cli/ check citations --root $PWD"));
    assert!(!joins("CARGO_TARGET_DIR=target ./bin/head-north land tests --select changed --only portable"));
    assert!(!joins("CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo clippy --all-targets"));
    assert!(!joins("CARGO_TARGET_DIR=target ./bin/head-north --code src/cli/ --no-build check citations --root $PWD"));
}

#[test]
fn a_probe_runs_its_head_north_checks_on_its_one_build_once_it_passed_and_its_other_checks_one_after_another_in_the_same_job() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().display();
    let build = format!("touch {path}/built");
    let head_north = format!("test -e {path}/built && echo checked");
    let first = format!("touch {path}/first-ran; sleep 0.2; rm {path}/first-ran; echo first");
    let second = format!("test ! -e {path}/first-ran && echo second");
    let parts = split(&ran(&on_one_build((&build, 30), &[(&head_north, 30)], &[(&first, 30), (&second, 30)])), 4);
    let outputs: Vec<(i32, &str)> = parts.iter().map(|part| part.as_ref().map_or((-1, ""), |part| (part.status, part.text.as_str()))).collect();
    assert_eq!(outputs, vec![(0, ""), (0, "checked\n"), (0, "first\n"), (0, "second\n")], "the head-north check sees the build and the second other check starts only after the first ended");
    let parts = split(&ran(&on_one_build(("exit 4", 30), &[(&head_north, 30)], &[("echo other", 30)])), 3);
    let statuses: Vec<i32> = parts.iter().map(|part| part.as_ref().map_or(-1, |part| part.status)).collect();
    assert_eq!(statuses, vec![4, 125, 0], "a failed build keeps its head-north checks from running and leaves the other checks theirs");
}
