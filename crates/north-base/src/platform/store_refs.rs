use std::sync::LazyLock;

const STORE: &str = "/nix/store/";
const HASH: usize = 32;
static STORE_LITERAL: LazyLock<regex::bytes::Regex> = LazyLock::new(|| regex::bytes::Regex::new(&regex::escape(STORE)).unwrap());

pub fn paths(bytes: &[u8]) -> Vec<String> {
    let named = |byte: &u8| byte.is_ascii_alphanumeric() || b"+-._?=/".contains(byte);
    let hashed = |byte: &u8| matches!(byte, b'0'..=b'9' | b'a'..=b'z') && !b"eotu".contains(byte);
    let mut found = Vec::new();
    let starts = std::iter::successors(STORE_LITERAL.find(bytes), |found| STORE_LITERAL.find_at(bytes, found.start() + 1));
    for at in starts.map(|found| found.start()) {
        let rest = &bytes[at + STORE.len()..];
        if rest.len() > HASH && rest[..HASH].iter().all(hashed) && rest[HASH] == b'-' {
            let name = rest[HASH + 1..].iter().take_while(|byte| named(byte)).count();
            found.push(String::from_utf8_lossy(&bytes[at..at + STORE.len() + HASH + 1 + name]).trim_end_matches('/').to_owned());
        }
    }
    found
}

pub fn root_of(path: &str) -> String {
    path.splitn(5, '/').take(4).collect::<Vec<_>>().join("/")
}

pub fn roots(bytes: &[u8]) -> Vec<String> {
    paths(bytes).iter().map(|path| root_of(path)).collect()
}

pub const RUNTIME_PROGRAMS: [&str; 2] = ["north", "verify-north"];
