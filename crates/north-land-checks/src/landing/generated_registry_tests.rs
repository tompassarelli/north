use std::path::Path;

use north_landing_gates::landing::generated_registry::{refusal, registries};
use north_landing_gates::landing::merge_set;

#[test]
fn a_hand_edit_of_a_generated_registry_is_refused_naming_its_source() {
    let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../").join(merge_set::DEFAULT_CONFIG);
    let registries = registries(&config).unwrap();
    assert!(registries.iter().any(|registry| registry.path == "docs/features.md"), "the real merge-set config no longer generates docs/features.md");

    for registry in &registries {
        let source = format!("{}unit.txt", registry.sources[0]);
        let lane = [registry.path.clone(), source.clone()];

        let error = refusal(&registries, &lane).unwrap_err();
        assert!(error.contains(&format!("{} is generated from", registry.path)), "{}: {error}", registry.path);
        assert!(error.contains(&format!("edit {source} instead")), "{}: {error}", registry.path);
        assert!(!error.contains(&format!("edit {} instead", registry.path)), "{}: {error}", registry.path);
    }
}
