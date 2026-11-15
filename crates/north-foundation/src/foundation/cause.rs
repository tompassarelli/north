//! The key that tells one alarm's cause from another. An alarm re-sent with a moved count, a fresh
//! runtime sha or a new canary session id is the same cause, so the fleet pages it once and holds the
//! rest for the digest: [`key`] lowercases the text, blanks every run of digits, and blanks a token
//! shaped like a commit sha. A token keeps its letters otherwise, so a first error line that changes
//! wording is still a new cause.

/// The cause key of `text`: trimmed and lowercased, with every run of digits replaced by `#` and
/// every commit-shaped token replaced by `#`.
///
/// A token is commit-shaped when it holds 7 to 40 ASCII hex characters and either a digit or at least
/// 12 characters, so `636c151b` and `22272a0b` share one key and a short word of hex letters such as
/// `efface` stays. A token is the longest run of ASCII alphanumerics; punctuation between tokens is
/// kept.
pub fn key(text: &str) -> String {
    let mut key = String::with_capacity(text.len());
    let mut token = String::new();
    for character in text.trim().chars() {
        if character.is_ascii_alphanumeric() {
            token.push(character);
            continue;
        }
        push_token(&mut key, &token);
        token.clear();
        key.push(character.to_ascii_lowercase());
    }
    push_token(&mut key, &token);
    key
}

fn push_token(key: &mut String, token: &str) {
    if commit_shaped(token) {
        key.push('#');
        return;
    }
    let mut digits = false;
    for character in token.chars() {
        if character.is_ascii_digit() {
            if !digits {
                key.push('#');
            }
            digits = true;
        } else {
            digits = false;
            key.push(character.to_ascii_lowercase());
        }
    }
}

fn commit_shaped(token: &str) -> bool {
    let length = token.len();
    (7..=40).contains(&length)
        && token.chars().all(|character| character.is_ascii_hexdigit())
        && (length >= 12 || token.chars().any(|character| character.is_ascii_digit()))
}
