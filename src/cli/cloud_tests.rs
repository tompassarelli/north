use super::*;

#[test]
fn cloud_curl_keeps_the_token_out_of_argv() {
    let args = curl_args(
        "POST",
        "https://api.anthropic.com/v1/sessions",
        Some(Path::new("/tmp/body")),
    );
    assert!(!args.iter().any(|arg| arg.contains("secret-token")));
    assert!(args.windows(2).any(|pair| pair == ["--config", "-"]));
    let config = curl_config("secret-token", "org-1");
    assert!(config.contains("header = \"Authorization: Bearer secret-token\"\n"));
    assert!(config.contains("header = \"x-organization-uuid: org-1\"\n"));
}
