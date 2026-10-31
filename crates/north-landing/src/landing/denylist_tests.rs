use super::*;
use crate::platform::process;

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

fn gzip(content: &[u8]) -> Vec<u8> {
    process::run_with_input_within("gzip", &["-c"], Some(content), std::time::Duration::from_secs(30)).unwrap().stdout
}

fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(!0u32, |crc, byte| {
        (0..8).fold(crc ^ u32::from(*byte), |crc, _| if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 })
    })
}

fn stored_zip(files: &[(&[u8], &[u8])]) -> Vec<u8> {
    let (mut local, mut central) = (Vec::new(), Vec::new());
    for (name, data) in files {
        let (crc, size, length) = (crc32(data).to_le_bytes(), (data.len() as u32).to_le_bytes(), (name.len() as u16).to_le_bytes());
        let common = [&20u16.to_le_bytes()[..], &[0; 8], &crc, &size, &size, &length, &[0; 2]].concat();
        let offset = (local.len() as u32).to_le_bytes();
        central.extend([&b"PK\x01\x02"[..], &20u16.to_le_bytes(), &common, &[0; 10], &offset, name].concat());
        local.extend([&b"PK\x03\x04"[..], &common, name, data].concat());
    }
    let count = (files.len() as u16).to_le_bytes();
    let end = [&b"PK\x05\x06"[..], &[0; 4], &count, &count].concat();
    let sizes = [(central.len() as u32).to_le_bytes(), (local.len() as u32).to_le_bytes()].concat();
    [local, central, end, sizes, vec![0; 2]].concat()
}

#[test]
fn denylist_archives_nested_past_the_depth_cap_fail_closed() {
    let mut content = b"owner notes\n".to_vec();
    for _ in 0..archive::DEPTH_CAP {
        content = gzip(&content);
    }
    let mut members = Vec::new();
    Unpacker::new(archive::DEPTH_CAP, archive::BYTE_CAP).members(b"deep.gz", &content, 0, &mut members).unwrap();
    assert_eq!(members.len(), archive::DEPTH_CAP);
    assert_eq!(members[0].content, b"owner notes\n");
    let error = Unpacker::new(archive::DEPTH_CAP, archive::BYTE_CAP).members(b"deep.gz", &gzip(&content), 0, &mut Vec::new()).unwrap_err();
    assert!(error.contains("nested past the archive depth cap of 8"), "{error}");
}

#[test]
fn denylist_archive_members_that_escape_the_staging_directory_fail_closed() {
    let zip = stored_zip(&[(&b"../escape.txt"[..], &b"owner notes\n"[..])]);
    assert_eq!(archive_extension(&zip), Some("zip"));
    let error = Unpacker::new(archive::DEPTH_CAP, archive::BYTE_CAP).members(b"evil.zip", &zip, 0, &mut Vec::new()).unwrap_err();
    assert!(error.contains("evil.zip holds a member that escapes the staging directory: ../escape.txt"), "{error}");
    let mut members = Vec::new();
    Unpacker::new(archive::DEPTH_CAP, archive::BYTE_CAP).members(b"fine.zip", &stored_zip(&[(&b"notes/a.txt"[..], &b"hi\n"[..])]), 0, &mut members).unwrap();
    assert_eq!(members, vec![archive::Member { path: b"fine.zip/notes/a.txt".to_vec(), depth: 1, content: b"hi\n".to_vec() }]);
}

#[test]
fn denylist_archives_that_repeat_a_member_name_fail_closed() {
    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path();
    std::fs::write(dir.join("a.txt"), "owner zebra-quokka-7731\n").unwrap();
    let archive = dir.join("dup.tar").to_string_lossy().into_owned();
    process::run_with_input_within("tar", &["-C", &dir.to_string_lossy(), "-cf", &archive, "a.txt"], None, std::time::Duration::from_secs(30)).unwrap();
    std::fs::write(dir.join("a.txt"), "benign\n").unwrap();
    process::run_with_input_within("tar", &["-C", &dir.to_string_lossy(), "-rf", &archive, "a.txt"], None, std::time::Duration::from_secs(30)).unwrap();
    let tar = std::fs::read(&archive).unwrap();
    let error = Unpacker::new(archive::DEPTH_CAP, archive::BYTE_CAP).members(b"dup.tar", &tar, 0, &mut Vec::new()).unwrap_err();
    assert!(error.contains("dup.tar holds the member a.txt more than once"), "{error}");
}

#[test]
fn denylist_archives_that_respell_a_member_path_fail_closed() {
    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path();
    std::fs::create_dir(dir.join("d")).unwrap();
    std::fs::write(dir.join("d/a.txt"), "owner zebra-quokka-7731\n").unwrap();
    let archive = dir.join("respelled.tar").to_string_lossy().into_owned();
    process::run_with_input_within("tar", &["-C", &dir.to_string_lossy(), "-cf", &archive, "d/a.txt"], None, std::time::Duration::from_secs(30)).unwrap();
    std::fs::write(dir.join("d/a.txt"), "benign\n").unwrap();
    process::run_with_input_within("tar", &["-C", &dir.to_string_lossy(), "-rf", &archive, "d/./a.txt"], None, std::time::Duration::from_secs(30)).unwrap();
    let tar = std::fs::read(&archive).unwrap();
    let error = Unpacker::new(archive::DEPTH_CAP, archive::BYTE_CAP).members(b"respelled.tar", &tar, 0, &mut Vec::new()).unwrap_err();
    assert!(error.contains("respelled.tar holds the member d/a.txt more than once"), "{error}");
}

fn document(text: &str) -> Vec<u8> {
    let xml = format!("<w:document><w:t>{text}, recorded with the release notes</w:t></w:document>\n");
    stored_zip(&[(&b"word/document.xml"[..], xml.as_bytes())])
}

fn scan_bundle(files: &[(&[u8], &[u8])]) -> BTreeSet<String> {
    let scratch = fixture();
    let root = scratch.path();
    write(root, "rules.toml", "title = \"fixture\"\n[[rules]]\nid = \"fake-private-term\"\nregex = '''zebra-quokka-7731'''\n");
    let base = commit_all(root, "rules");
    std::fs::write(root.join("bundle.zip"), stored_zip(files)).unwrap();
    let head = commit_all(root, "add document bundle");
    scan_range(root, &root.join("rules.toml"), &base, &head, None).unwrap()
}

#[test]
fn denylist_findings_inside_a_zipped_document_name_the_nested_member() {
    let findings = scan_bundle(&[(&b"notes.docx"[..], &document("owner zebra-quokka-7731"))]);
    assert_eq!(findings, BTreeSet::from(["fake-private-term in bundle.zip/notes.docx/word/document.xml".to_owned()]));
}

#[test]
fn denylist_clean_zipped_documents_with_dots_in_their_names_publish() {
    let findings = scan_bundle(&[(&b"docs/summary.docx"[..], &document("owner: the maintainer")), (&b"docs/q3..q4-review.docx"[..], &document("quarterly review by the maintainer"))]);
    assert_eq!(findings, BTreeSet::new());
}
