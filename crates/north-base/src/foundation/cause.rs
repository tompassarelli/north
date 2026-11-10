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

#[cfg(test)]
mod tests {
    use super::key;

    #[test]
    fn a_fresh_sha_canary_id_or_count_keeps_one_key() {
        let first = key("north-runtime-follow.service failed to start 13 times in the last hour: runtime 636c151b, canary-opencode-4101624");
        for changed in [
            "north-runtime-follow.service failed to start 7 times in the last hour: runtime 636c151b, canary-opencode-4101624",
            "north-runtime-follow.service failed to start 13 times in the last hour: runtime 22272a0b, canary-opencode-4123229",
            "NORTH-RUNTIME-FOLLOW.SERVICE FAILED TO START 13 TIMES IN THE LAST HOUR: RUNTIME 636C151B, CANARY-OPENCODE-4101624",
        ] {
            assert_eq!(key(changed), first, "{changed}");
        }
    }

    #[test]
    fn changed_words_are_a_new_cause_and_a_short_hex_word_stays() {
        assert_ne!(
            key("runtime failed on 636c151b"),
            key("runtime timed out on 636c151b"),
            "a changed first error line is a new cause"
        );
        assert_eq!(key("efface the file"), "efface the file");
        assert_eq!(key("deface"), "deface");
    }

    #[test]
    fn digits_and_punctuation_are_blanked_as_before() {
        assert_eq!(key("still, 12 min: disk 3.1 GiB free on /nix/store"), "still, # min: disk #.# gib free on /nix/store");
        assert_eq!(key("north#1031"), "north##");
    }
}
