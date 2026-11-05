use super::*;

#[test]
fn mirror_path_names_the_repository_the_origin_url_ends_in() {
    for (url, name) in [
        ("git@github.com:tompassarelli/north.git", "north"),
        ("https://github.com/tompassarelli/makgora-client", "makgora-client"),
        ("/tmp/fixture/remote.git/", "remote"),
    ] {
        let path = mirror_path(url).unwrap();
        assert_eq!(path.file_name().unwrap().to_string_lossy(), format!("{name}.git"), "{url}");
    }
    assert_eq!(mirror_path(""), None);
}
