use super::*;

fn write(root: &Path, path: &str, text: &str) {
    std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
    std::fs::write(root.join(path), text).unwrap();
}

fn commit_all(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]).unwrap();
    git(root, &["-c", "commit.gpgsign=false", "commit", "-qm", message]).unwrap();
    git(root, &["rev-parse", "HEAD"]).unwrap().trim().to_owned()
}

fn fixture() -> tempfile::TempDir {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    git(root, &["init", "-q", "-b", "main"]).unwrap();
    git(root, &["config", "user.name", "fixture"]).unwrap();
    git(root, &["config", "user.email", "fixture@example.invalid"]).unwrap();
    scratch
}

#[test]
fn denylist_published_entries_skip_deletions_and_mark_gitlinks_and_trees() {
    let raw = b":000000 100644 0000 aaaa A\0harness/skills/new.md\0:100644 100644 bbbb cccc M\0edited.md\0\
:100644 000000 dddd 0000 D\0gone.md\0:000000 160000 0000 eeee A\0vendor/sub\0:000000 040000 0000 ffff A\0empty-dir\0";
    assert_eq!(
        published_entries(raw),
        vec![
            Blob { path: b"harness/skills/new.md".to_vec(), oid: "aaaa".into(), file: true },
            Blob { path: b"edited.md".to_vec(), oid: "cccc".into(), file: true },
            Blob { path: b"vendor/sub".to_vec(), oid: "eeee".into(), file: false },
            Blob { path: b"empty-dir".to_vec(), oid: "ffff".into(), file: false },
        ]
    );
}

#[test]
fn denylist_layers_never_hold_a_path_and_one_beneath_it() {
    let blob = |path: &str, oid: &str| Blob { path: path.as_bytes().to_vec(), oid: oid.into(), file: true };
    let blobs: BTreeSet<Blob> = [blob("a", "1"), blob("a", "4"), blob("a/b", "2"), blob("ab", "3")].into_iter().collect();
    let oids: Vec<Vec<&str>> = layers(&blobs).iter().map(|layer| layer.iter().map(|b| b.oid.as_str()).collect()).collect();
    assert_eq!(oids, vec![vec!["1", "3"], vec!["4"], vec!["2"]]);
}

#[test]
fn denylist_staged_paths_refuse_escaping_components() {
    assert!(staged_path(b"harness/skills/a.md").is_ok());
    for path in [&b"../x"[..], b"a/../../x", b"/abs", b"a//b", b"./a"] {
        assert!(staged_path(path).is_err(), "{}", String::from_utf8_lossy(path));
    }
}

#[test]
fn denylist_ref_segments_follow_nested_tags() {
    let scratch = fixture();
    let root = scratch.path();
    write(root, "a.txt", "a\n");
    commit_all(root, "base");
    git(root, &["-c", "tag.gpgsign=false", "tag", "-a", "inner", "-m", "inner note"]).unwrap();
    git(root, &["-c", "tag.gpgsign=false", "tag", "-a", "outer", "-m", "outer note", "inner"]).unwrap();
    let texts: Vec<String> = ref_segments(root, "outer").into_iter().skip(1).map(|s| s.text).collect();
    assert_eq!(texts.len(), 2);
    assert!(texts[0].contains("outer note") && texts[1].contains("inner note"));
}

#[test]
fn denylist_archive_extension_reads_signatures_not_names() {
    assert_eq!(archive_extension(b"PK\x03\x04rest"), Some("zip"));
    assert_eq!(archive_extension(b"\x1f\x8b\x08\x00"), Some("gz"));
    assert_eq!(archive_extension(b"\x28\xb5\x2f\xfdxx"), Some("zst"));
    let mut tar = vec![0u8; 300];
    tar[257..262].copy_from_slice(b"ustar");
    assert_eq!(archive_extension(&tar), Some("tar"));
    assert_eq!(archive_extension(b"plain text"), None);
}

#[test]
fn denylist_text_copies_take_a_neutral_name_in_the_same_directory() {
    assert_eq!(neutral_name(b"nix/north-fleet.target", "0123456789abcdef"), b"nix/0123456789ab.txt".to_vec());
    assert_eq!(neutral_name(b"pkg.zip.sha256", "0123456789abcdef"), b"0123456789ab.txt".to_vec());
    assert!(is_text(b"plain\ntext"));
    assert!(!is_text(b"bin\0ary"));
}

#[test]
fn denylist_scan_takes_a_config_or_a_south_checkout_but_not_both() {
    let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    let south = parse(&args(&["scan", "--south", "/s", "a", "b", "--ref", "claude/x"])).unwrap();
    assert_eq!(south.config, Source::South("/s".into()));
    assert_eq!(south.ref_name.as_deref(), Some("claude/x"));
    assert_eq!(parse(&args(&["scan", "--config", "/c", "a", "b"])).unwrap().config, Source::Config("/c".into()));
    assert_eq!(parse(&args(&["scan", "--config", "/c", "--south", "/s", "a", "b"])), None);
    assert_eq!(parse(&args(&["scan", "a", "b"])), None);
}

#[test]
fn denylist_south_rules_come_from_origin_main_not_the_checkout() {
    let scratch = fixture();
    let origin = scratch.path();
    write(origin, "privacy/public-denylist.toml", "stale = true\n");
    commit_all(origin, "old rules");
    let south = tempfile::tempdir().unwrap();
    let main = south.path().join("main");
    git(south.path(), &["clone", "-q", &origin.to_string_lossy(), "main"]).unwrap();
    write(origin, "privacy/public-denylist.toml", "fresh = true\n");
    commit_all(origin, "new rules");
    let rules = south_denylist(south.path()).unwrap();
    assert_eq!(std::fs::read_to_string(rules.path()).unwrap(), "fresh = true\n");
    assert_eq!(std::fs::read_to_string(main.join("privacy/public-denylist.toml")).unwrap(), "stale = true\n");
    git(origin, &["rm", "-q", "privacy/public-denylist.toml"]).unwrap();
    commit_all(origin, "drop rules");
    assert!(south_denylist(south.path()).unwrap_err().contains("missing"));
}
