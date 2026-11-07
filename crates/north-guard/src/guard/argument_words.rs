pub(super) fn is_redirect(word: &str) -> bool {
    word.trim_start_matches(|c: char| c.is_ascii_digit()).starts_with(['>', '<'])
}

pub(super) fn is_redirect_operator(word: &str) -> bool {
    is_redirect(word) && word.trim_start_matches(|c: char| c.is_ascii_digit()).chars().all(|c| matches!(c, '>' | '<'))
}

pub(super) fn is_plain_argument(word: &str) -> bool {
    !word.starts_with('-') && !is_redirect(word) && !word.bytes().all(|b| b.is_ascii_digit())
}
