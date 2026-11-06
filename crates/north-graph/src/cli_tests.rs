use crate::cli::run;

fn graph(database: &std::path::Path, arguments: &[&str]) -> u8 {
    let mut command: Vec<String> = arguments.iter().map(ToString::to_string).collect();
    command.extend(["--db".to_string(), database.display().to_string(), "--writer".to_string(), "test".to_string()]);
    run(&command)
}

#[test]
fn the_five_verbs_run_against_a_database_file_and_validate_exits_zero_when_sound() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("graph.db");
    assert_eq!(graph(&database, &["tell", "north#669", ":blocks", "north#670"]), 0);
    assert_eq!(graph(&database, &["show", "north#669"]), 0);
    assert_eq!(graph(&database, &["retract", "north#669", ":blocks", "north#670"]), 0);
    assert_eq!(graph(&database, &["retract", "north#669", ":blocks", "north#670"]), 0);
    let query = r#"{"find":"o","rules":[{"head":{"rel":"o","args":[{"var":"c"}]},"body":[{"rel":"occurrence","args":[{"var":"c"},{"var":"a"},{"var":"p"}]}]}]}"#;
    assert_eq!(graph(&database, &["ask", query]), 0);
    assert_eq!(graph(&database, &["validate"]), 0);
}

#[test]
fn bad_input_exits_nonzero_without_touching_the_store() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("graph.db");
    assert_eq!(graph(&database, &["tell", "only", "two"]), 2);
    assert_eq!(graph(&database, &["ask", "{not json"]), 1);
    assert_eq!(graph(&database, &["nonsense"]), 2);
    assert_eq!(run(&[]), 2);
}
