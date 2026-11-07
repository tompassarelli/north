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
fn denylist_archive_members_that_escape_the_staging_directory_fail_closed() {
    let zip = stored_zip(&[(&b"../escape.txt"[..], &b"owner notes\n"[..])]);
    assert_eq!(archive_extension(&zip), Some("zip"));
    let error = Unpacker::new(archive::DEPTH_CAP, archive::BYTE_CAP).members(b"evil.zip", &zip, 0, &mut Vec::new()).unwrap_err();
    assert!(error.contains("evil.zip holds a member that escapes the staging directory: ../escape.txt"), "{error}");
    let mut members = Vec::new();
    Unpacker::new(archive::DEPTH_CAP, archive::BYTE_CAP).members(b"fine.zip", &stored_zip(&[(&b"notes/a.txt"[..], &b"hi\n"[..])]), 0, &mut members).unwrap();
    assert_eq!(members, vec![archive::Member { path: b"fine.zip/notes/a.txt".to_vec(), depth: 1, content: b"hi\n".to_vec() }]);
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
fn denylist_a_fifo_in_a_tar_is_an_empty_member_and_the_files_beside_it_are_still_scanned() {
    let scratch = tempfile::tempdir().unwrap();
    let tree = scratch.path().join("tree");
    std::fs::create_dir_all(&tree).unwrap();
    std::fs::write(tree.join("notes.txt"), "owner zebra-quokka-7731\n").unwrap();
    assert!(std::process::Command::new("mkfifo").arg(tree.join("fifo")).status().unwrap().success());
    let tar = std::process::Command::new("tar").arg("-C").arg(scratch.path()).args(["-cf", "-", "tree"]).output().unwrap();
    assert!(tar.status.success());
    let mut members = Vec::new();
    Unpacker::new(archive::DEPTH_CAP, archive::BYTE_CAP).members(b"fixture.tar", &tar.stdout, 0, &mut members).unwrap();
    let found: Vec<(String, Vec<u8>)> = members.into_iter().map(|member| (String::from_utf8(member.path).unwrap(), member.content)).collect();
    assert_eq!(found, [("fixture.tar/tree/fifo".to_owned(), Vec::new()), ("fixture.tar/tree/notes.txt".to_owned(), b"owner zebra-quokka-7731\n".to_vec())]);
}
