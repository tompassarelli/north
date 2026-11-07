use std::sync::LazyLock;

use regex::bytes::Regex;

use super::host_paths::is_test_code;

static WALL_CLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"sleep\(|Instant::now\(|\.elapsed\(\)").expect("wall clock pattern"));

pub fn find<'a>(path: &[u8], line: &'a [u8]) -> Option<&'a [u8]> {
    if !path.ends_with(b".rs") || !is_test_code(path) {
        return None;
    }
    WALL_CLOCK.find(line).map(|found| found.as_bytes())
}
