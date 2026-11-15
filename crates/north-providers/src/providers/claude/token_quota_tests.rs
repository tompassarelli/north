use super::curl_config;


#[test]
fn a_token_that_could_add_curl_config_lines_is_refused_without_echoing_it() {
    let config = curl_config("setup-token").unwrap();
    assert_eq!(config.lines().count(), 4);
    for hostile in ["", "two words", "line\nheader = \"x: y\"", "quote\"", "back\\slash"] {
        let refusal = curl_config(hostile).unwrap_err();
        assert!(!refusal.contains("quote") && !refusal.contains("slash"), "{refusal}");
    }
}
