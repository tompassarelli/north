use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use super::pack;

const SOURCE: &str = "NORTH_TEST_PACK_SOURCE";
const OUTPUT: &str = "NORTH_TEST_PACK_OUTPUT";

#[test]
fn a_scratch_directory_packs_for_the_far_side_under_a_path_without_tar() {
    if let (Some(source), Some(output)) = (std::env::var_os(SOURCE), std::env::var_os(OUTPUT)) {
        let source = PathBuf::from(source);
        let packed = pack(source.parent().unwrap(), source.file_name().unwrap()).unwrap();
        std::fs::write(output, packed).unwrap();
        return;
    }
    let work = tempfile::tempdir().unwrap();
    let scratch = work.path().join("src/north-opencode-box");
    let deep = scratch.join(format!("{0}/{0}/skills", "a-long-directory-name".repeat(2)));
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(scratch.join("instructions.md"), "go").unwrap();
    std::fs::write(deep.join("SKILL.md"), vec![7u8; 1500]).unwrap();
    std::fs::set_permissions(scratch.join("instructions.md"), std::fs::Permissions::from_mode(0o751)).unwrap();
    std::os::unix::fs::symlink("instructions.md", scratch.join("link")).unwrap();
    std::fs::create_dir_all(scratch.join("empty")).unwrap();
    let archive = work.path().join("scratch.tar");

    let name = std::thread::current().name().unwrap().to_owned();
    let child = Command::new(std::env::current_exe().unwrap()).args(["--exact", &name]).env_clear().env("PATH", "/nonexistent").env(SOURCE, &scratch).env(OUTPUT, &archive).output().unwrap();
    assert!(child.status.success(), "{}", String::from_utf8_lossy(&child.stderr));

    let far = work.path().join("far");
    std::fs::create_dir(&far).unwrap();
    let extracted = Command::new("tar").arg("-C").arg(&far).arg("-xpf").arg(&archive).output().unwrap();
    assert!(extracted.status.success(), "{}", String::from_utf8_lossy(&extracted.stderr));
    let landed = far.join("north-opencode-box");
    assert_eq!(std::fs::read_to_string(landed.join("instructions.md")).unwrap(), "go");
    assert_eq!(std::fs::metadata(landed.join("instructions.md")).unwrap().permissions().mode() & 0o777, 0o751);
    assert_eq!(std::fs::read(landed.join(deep.strip_prefix(&scratch).unwrap()).join("SKILL.md")).unwrap(), vec![7u8; 1500]);
    assert_eq!(std::fs::read_link(landed.join("link")).unwrap(), PathBuf::from("instructions.md"));
    assert!(landed.join("empty").is_dir());
}
