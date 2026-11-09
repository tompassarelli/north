use std::path::Path;
use std::process::Command;

use north_ops::foundation::shim::write_script;
use north_ops::platform::rolling_issue::{Breaches, Gh, alarm};

const TEST: &str = "a_second_failure_of_the_same_unit_updates_its_open_issue_instead_of_filing_another";
const TITLE: &str = "Harness services failing on whiterabbit";

const GH: &str = r#"
case "$*" in
  *"issue create"*) printf 'create\n' >> "$NORTH_TEST_GH_DIR/log"; touch "$NORTH_TEST_GH_DIR/created" ;;
  *"issue edit"*) printf 'edit\n' >> "$NORTH_TEST_GH_DIR/log" ;;
  *"issue list"*--search*) printf '[]\n' ;;
  *"issue list"*)
    if [ -e "$NORTH_TEST_GH_DIR/created" ]; then
      printf '[{"number":7,"title":"%s","body":"old"}]\n' "$NORTH_TEST_GH_TITLE"
    else
      printf '[]\n'
    fi ;;
  *) printf '{}\n' ;;
esac
"#;

#[test]
fn a_second_failure_of_the_same_unit_updates_its_open_issue_instead_of_filing_another() {
    let Some(directory) = std::env::var_os("NORTH_TEST_GH_DIR") else {
        let directory = tempfile::tempdir().unwrap();
        let bin = directory.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        write_script(&bin.join("gh"), GH);
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env("PATH", path)
            .env("NORTH_TEST_GH_DIR", directory.path())
            .env("NORTH_TEST_GH_TITLE", TITLE)
            .status()
            .unwrap();
        assert!(status.success(), "the replay exited {status}");
        assert!(directory.path().join("ran").is_file(), "the replay ran no test");
        return;
    };
    let directory = Path::new(&directory);
    std::fs::write(directory.join("ran"), "").unwrap();

    let mut gh = Gh::of("tompassarelli/north", &[]);
    let mut breaches = Breaches::default();
    alarm(&mut gh, &mut breaches, TITLE, Some(&body("2")), "closed").unwrap();
    alarm(&mut gh, &mut breaches, TITLE, Some(&body("2")), "closed").unwrap();
    alarm(&mut gh, &mut breaches, TITLE, Some(&body("50")), "closed").unwrap();

    let log = std::fs::read_to_string(directory.join("log")).unwrap();
    assert_eq!(log.lines().filter(|line| *line == "create").count(), 1, "the second reading filed another issue:\n{log}");
    assert_eq!(log.lines().filter(|line| *line == "edit").count(), 1, "the second reading did not update the open issue:\n{log}");
}

fn body(streak: &str) -> String {
    format!("Class: a harness unit failing on whiterabbit\n\n- `fleet-tick.service`: fleet-tick.service failed {streak} runs in a row.\n")
}
