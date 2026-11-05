use super::*;

#[test]
fn the_process_table_is_read_from_stat_and_cmdline() {
    let root = tempfile::tempdir().unwrap();
    for (pid, stat, cmdline) in [
        ("12", "12 (node (x)) S 7 12 12 0 -1\n", &b"claude\0-p\0hi\0"[..]),
        ("13", "13 (bash) S 12 13 13 0 -1\n", b""),
        ("self", "ignored", b"x\0"),
    ] {
        let dir = root.path().join(pid);
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("stat"), stat).unwrap();
        fs::write(dir.join("cmdline"), cmdline).unwrap();
    }
    fs::create_dir(root.path().join("14")).unwrap();
    let table = process_table(root.path());
    assert_eq!(table.keys().copied().collect::<Vec<_>>(), [12, 13]);
    assert_eq!((table[&12].comm.as_str(), table[&12].ppid, table[&12].argv.clone()), ("node (x)", 7, vec!["claude".to_owned(), "-p".into(), "hi".into()]));
    assert!(table[&13].argv.is_empty());
}
