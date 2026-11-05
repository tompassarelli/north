#[test]
fn the_repository_structure_toml_loads() {
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../structure.toml")).unwrap();
    super::config::parse(Some(&text), Default::default()).unwrap();
}
