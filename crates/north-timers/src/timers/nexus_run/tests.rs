use super::plan::select_files;

#[test]
fn only_tracked_and_unignored_files_that_exist_and_are_not_private_are_sent() {
    let source = tempfile::tempdir().expect("tempdir");
    for name in [".gitignore", "a.txt", "dir/b.txt", "new.txt", "sp ace.txt", "sub/s.txt", ".env", ".env.local", "node_modules/x/y.js", ".ssh/key", "sub/.env.d/z", "sub/.git/config"] {
        let path = source.path().join(name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, "x").expect("file");
    }
    let listing = b"sub/s.txt\0a.txt\0.env\0.env.local\0node_modules/x/y.js\0.ssh/key\0sub/.env.d/z\0sub/.git/config\0gone.txt\0sp ace.txt\0a.txt\0dir/b.txt\0new.txt\0.gitignore\0";
    let selected: Vec<String> = select_files(listing, source.path()).iter().map(|name| String::from_utf8_lossy(name).into_owned()).collect();
    assert_eq!(selected, [".gitignore", "a.txt", "dir/b.txt", "new.txt", "sp ace.txt", "sub/s.txt"]);
}

