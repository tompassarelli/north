use std::path::Path;

use north_landing_gates::landing::generated_registry::{refusal, registries};
use north_landing_gates::landing::merge_set::{self, Kind};

#[test]
fn a_hand_edit_of_a_tracked_generated_file_is_left_to_the_lander_and_an_untracked_index_is_refused_naming_its_source() {
    let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../").join(merge_set::DEFAULT_CONFIG);
    let generated: Vec<_> = merge_set::read_rules(&config).unwrap().into_iter().filter(|rule| matches!(rule.kind, Kind::Regenerate { .. })).map(|rule| rule.path).collect();
    for tracked in ["structure.toml", "docs/rationale/README.md", "src/cli/reference.txt"] {
        assert!(generated.iter().any(|path| path == tracked), "the real merge-set config no longer regenerates {tracked}");
        assert_eq!(refusal(&registries(), &[tracked.to_owned(), "docs/rationale/unit.md".to_owned()]), Ok(()), "{tracked}");
    }

    let error = refusal(&registries(), &["docs/features.md".to_owned(), "docs/features/unit.md".to_owned()]).unwrap_err();
    assert!(error.contains("docs/features.md is generated from"), "{error}");
    assert!(error.contains("edit docs/features/unit.md instead"), "{error}");
}
