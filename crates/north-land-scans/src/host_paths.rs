use std::ops::Range;
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

pub(crate) fn is_test_code(path: &[u8]) -> bool {
    TEST_CODE.is_match(path) && !RECORDED_LOGS.iter().any(|prefix| path.starts_with(prefix))
}

pub(crate) fn is_guest_file(path: &[u8]) -> bool {
    is_test_code(path) && path.ends_with(b".test.nix")
}

/// A NixOS VM test describes guests, so a path inside one of its strings is the guest's path when
/// the guest runs that text. Only its host code names a host path: a bare path expression, or the
/// `${...}` the host evaluates inside a string.
pub(crate) struct GuestText {
    line_starts: Vec<usize>,
    spans: Vec<Range<usize>>,
}

impl GuestText {
    pub(crate) fn of(source: &[u8]) -> Self {
        let line_starts = std::iter::once(0)
            .chain(
                source
                    .iter()
                    .enumerate()
                    .filter(|(_, byte)| **byte == b'\n')
                    .map(|(index, _)| index + 1),
            )
            .collect();
        Self {
            line_starts,
            spans: string_contents(source),
        }
    }

    pub(crate) fn admits(&self, line: usize, column: usize) -> bool {
        let Some(start) = line
            .checked_sub(1)
            .and_then(|index| self.line_starts.get(index))
        else {
            return false;
        };
        self.spans
            .iter()
            .any(|span| span.contains(&(start + column)))
    }
}

pub fn find<'a>(
    path: &[u8],
    line: &'a [u8],
    line_number: usize,
    guest: Option<&GuestText>,
) -> Option<&'a [u8]> {
    if !is_test_code(path) {
        return None;
    }
    HOST_PATH
        .find_iter(line)
        .find(|found| !guest.is_some_and(|text| text.admits(line_number, found.start())))
        .map(|found| found.as_bytes())
}

fn string_contents(source: &[u8]) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut index = 0;
    while index < source.len() {
        match source[index] {
            b'#' => index = line_comment(source, index),
            b'/' if source.get(index + 1) == Some(&b'*') => index = block_comment(source, index),
            b'"' => index = double_quoted(source, index + 1, &mut spans),
            b'\'' if source.get(index + 1) == Some(&b'\'') => {
                index = indented(source, index + 2, &mut spans)
            }
            _ => index += 1,
        }
    }
    spans
}

fn add(spans: &mut Vec<Range<usize>>, start: usize, end: usize) {
    if start < end {
        spans.push(start..end);
    }
}

fn double_quoted(source: &[u8], mut index: usize, spans: &mut Vec<Range<usize>>) -> usize {
    let mut start = index;
    let mut escaped = false;
    while index < source.len() {
        let byte = source[index];
        if escaped {
            escaped = false;
            index += 1;
        } else if byte == b'\\' {
            escaped = true;
            index += 1;
        } else if byte == b'"' {
            add(spans, start, index);
            return index + 1;
        } else if byte == b'$' && source.get(index + 1) == Some(&b'{') {
            add(spans, start, index);
            index = interpolation(source, index + 2);
            start = index;
        } else {
            index += 1;
        }
    }
    add(spans, start, index);
    index
}

fn indented(source: &[u8], mut index: usize, spans: &mut Vec<Range<usize>>) -> usize {
    let mut start = index;
    while index < source.len() {
        if source[index..].starts_with(b"''") {
            match source.get(index + 2) {
                Some(b'\'') | Some(b'$') => index += 3,
                Some(b'\\') => index = (index + 4).min(source.len()),
                _ => {
                    add(spans, start, index);
                    return index + 2;
                }
            }
        } else if source[index..].starts_with(b"${") {
            add(spans, start, index);
            index = interpolation(source, index + 2);
            start = index;
        } else {
            index += 1;
        }
    }
    add(spans, start, index);
    index
}

fn interpolation(source: &[u8], mut index: usize) -> usize {
    let mut depth = 1;
    while index < source.len() {
        match source[index] {
            b'{' => {
                depth += 1;
                index += 1;
            }
            b'}' => {
                depth -= 1;
                index += 1;
                if depth == 0 {
                    return index;
                }
            }
            b'"' => index = double_quoted(source, index + 1, &mut Vec::new()),
            b'\'' if source.get(index + 1) == Some(&b'\'') => {
                index = indented(source, index + 2, &mut Vec::new())
            }
            b'#' => index = line_comment(source, index),
            b'/' if source.get(index + 1) == Some(&b'*') => index = block_comment(source, index),
            _ => index += 1,
        }
    }
    index
}

fn line_comment(source: &[u8], index: usize) -> usize {
    source[index..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(source.len(), |offset| index + offset)
}

fn block_comment(source: &[u8], index: usize) -> usize {
    source[index + 2..]
        .windows(2)
        .position(|pair| pair == b"*/")
        .map_or(source.len(), |offset| index + 2 + offset + 2)
}
