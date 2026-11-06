use std::sync::LazyLock;

use regex::bytes::Regex;

static TEST_FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(^|/)tests?/|(^|/|[._])tests?\.[^/]*$|(^|/)test_[^/]*\.py$").expect("test file pattern")
});

pub(crate) fn is_test_file(path: impl AsRef<[u8]>) -> bool {
    TEST_FILE.is_match(path.as_ref())
}
