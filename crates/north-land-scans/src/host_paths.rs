use std::sync::LazyLock;

use regex::bytes::Regex;

static TEST_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(^|/)(tests?|fixtures)/|(^|[/._])tests?\.[^/]*$").expect("test code pattern")
});
static HOST_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"/nix/store/[0-9a-z]{32}|/run/current-system|/nix/var|\.nix-profile")
        .expect("host path pattern")
});

const RECORDED_LOGS: [&[u8]; 1] = [b"tests/fixtures/"];

pub fn find<'a>(path: &[u8], line: &'a [u8]) -> Option<&'a [u8]> {
    if !TEST_CODE.is_match(path) || RECORDED_LOGS.iter().any(|prefix| path.starts_with(prefix)) {
        return None;
    }
    HOST_PATH.find(line).map(|found| found.as_bytes())
}
