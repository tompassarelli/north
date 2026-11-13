use std::path::Path;

use super::{Buck2, TEST_RULE};
use crate::cargo::Cargo;
use crate::{BuildSystem, Invocation, Shell};

const TEST_BUILD: &str = r#"set -e; query=$1; shift; labels=$("$@" uquery "$query"); "$@" build --remote-only --show-json-output $labels"#;

pub(super) fn request(side: &Buck2, root: &Path) -> Result<Invocation, String> {
    if !side.tests {
        return Cargo.test_build(root);
    }
    let query = format!("kind(\"{TEST_RULE}\", //...)");
    let buck2 = side.invocation(root, Shell::LandShell, side.words(&[])).words;
    let words = ["bash", "-c", TEST_BUILD, "bash", &query].map(str::to_owned).into_iter().chain(buck2).collect();
    Ok(side.invocation(root, Shell::Inherited, words))
}
