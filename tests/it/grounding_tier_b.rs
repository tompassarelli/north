use std::process::Command;

fn grounding(arguments: &[&str]) -> String {
    let state = tempfile::tempdir().unwrap();
    let handoffs = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .arg("grounding")
        .args(arguments)
        .env("NORTH_GROUNDING_STATE", state.path())
        .env("NORTH_GROUNDING_HANDOFFS", handoffs.path())
        .env("NORTH_GROUNDING_CONVO_DB", state.path().join("missing.db"))
        .env("NORTH_GROUNDING_NORTH", state.path().join("missing"))
        .env("NORTH_GROUNDING_SOUTH", state.path().join("missing"))
        .env("PATH", "/nonexistent")
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn tier_b_without_a_hit_prints_the_ask_format_naming_what_was_searched() {
    let out = grounding(&["--tier", "B", "--query", "didn't we already build the zebra quokka harmonica thing"]);
    assert!(out.contains("searched "), "{out}");
    assert!(out.contains(" for '") && out.contains("zebra"), "{out}");
    assert!(out.contains("this one?"), "{out}");
}

#[test]
fn a_past_referring_query_is_grounded_or_names_the_tier_b_command() {
    let out = grounding(&["--query", "didn't we already set up the river gauge cache?"]);
    assert!(out.contains("--tier B") || out.contains("handoffs/gauge"), "{out}");
}
