use super::{canonical, land_arguments};

fn clippy_land_line() -> String {
    let config = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.safe-push")).unwrap();
    config.lines().filter_map(|line| line.trim().split_once(" :: ")).map(|(_, command)| command.to_owned()).find(|command| command.contains("cargo clippy")).unwrap()
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn a_session_clippy_over_the_lanes_changed_packages_has_the_land_clippy_lines_verdict_and_any_other_package_set_or_flag_does_not() {
    let environment = [("SAFE_PUSH_LAND_CHANGED".to_owned(), "-p north-a -p north-b".to_owned())];
    let land = canonical(&land_arguments(&clippy_land_line(), &environment).unwrap()).unwrap();
    let own = |text: &str| canonical(&words(text));
    assert_eq!(own("clippy -p north-b --all-targets --package north-a -- -D warnings").as_deref(), Some(land.as_str()));
    for differs in ["clippy --all-targets -p north-a -- -D warnings", "clippy --all-targets -p north-a -p north-b -p north-c -- -D warnings", "clippy --all-targets --workspace -- -D warnings", "clippy -p north-a -p north-b -- -D warnings", "clippy --all-targets -p north-a -p north-b --features x -- -D warnings", "clippy --all-targets -p north-a -p north-b"] {
        assert_ne!(own(differs).as_deref(), Some(land.as_str()), "{differs}");
    }
    let workspace = canonical(&land_arguments(&clippy_land_line(), &[]).unwrap()).unwrap();
    assert_eq!(own("clippy --workspace --all-targets --locked --offline -- -D warnings").as_deref(), Some(workspace.as_str()));
}
