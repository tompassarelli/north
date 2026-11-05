use super::*;

#[test]
fn the_key_is_the_first_sixteen_hex_of_the_origin_hash_the_bash_lock_uses() {
    assert_eq!(key("https://github.com/tompassarelli/north.git").len(), 16);
    assert_eq!(key("https://github.com/tompassarelli/north.git"), "de2e0e8479546ba9");
    assert_eq!(key("https://github.com/tompassarelli/north"), "de2e0e8479546ba9");
    assert_eq!(key("https://github.com/tompassarelli/north/"), "de2e0e8479546ba9");
}
