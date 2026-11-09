use std::cell::RefCell;
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::{Claude, Failure, Install, Network, Vendor, install};

const CLAUDE_SCRIPT: &str = "#!/bin/sh\necho '9.9.9 (Claude Code)'\n";
struct Fake {
    payload: &'static str,
    manifest: String,
    calls: RefCell<Vec<String>>,
}

impl Network for Fake {
    fn text(&self, url: &str, _: bool) -> Result<String, String> {
        self.calls.borrow_mut().push(url.to_owned());
        if url.ends_with("/manifest.json") || url.contains("/releases") { Ok(self.manifest.clone()) } else { Ok("9.9.9\n".to_owned()) }
    }

    fn download(&self, url: &str, destination: &Path) -> Result<(), String> {
        self.calls.borrow_mut().push(url.to_owned());
        fs::write(destination, self.payload).map_err(|error| error.to_string())
    }

    fn unzstd(&self, input: &Path, output: &Path) -> Result<(), String> {
        fs::copy(input, output).map(|_| ()).map_err(|error| error.to_string())
    }

    fn untar_member(&self, archive: &Path, _: &str, output: &Path) -> Result<(), String> {
        fs::copy(archive, output).map(|_| ()).map_err(|error| error.to_string())
    }

    fn untar_into(&self, archive: &Path, directory: &Path, _: u32) -> Result<(), String> {
        Err(format!("{} into {}", archive.display(), directory.display()))
    }
}

fn claude_fake(corrupt: bool) -> Fake {
    let sum = if corrupt { "0".repeat(64) } else { format!("{:x}", Sha256::digest(CLAUDE_SCRIPT)) };
    Fake { payload: CLAUDE_SCRIPT, manifest: format!(r#"{{"version":"9.9.9","platforms":{{"linux-x64":{{"checksum":"{sum}"}}}}}}"#), calls: RefCell::default() }
}

fn run(vendor: &dyn Vendor, network: &Fake, root: &Path, probe: &dyn Fn(&Path) -> bool, version: &str) -> Result<String, Failure> {
    install(&Install { vendor, network, root, probe }, version)
}

#[test]
fn a_failing_probe_refuses_promotion_and_leaves_nothing_behind() {
    let root = tempfile::tempdir().unwrap();
    let refused = run(&Claude, &claude_fake(false), root.path(), &|_| false, "9.9.9").err().unwrap();
    assert_eq!((refused.0.as_str(), refused.1), ("claude-runtime-update: contract probe failed; refusing promotion", 1));
    assert!(!root.path().join("current").exists() && !root.path().join("versions/9.9.9").exists());
    let leftover: Vec<_> = fs::read_dir(root.path()).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| name.starts_with(".stage.")).collect();
    assert!(leftover.is_empty(), "{leftover:?}");
}

#[test]
fn a_checksum_mismatch_or_invalid_version_never_installs() {
    let root = tempfile::tempdir().unwrap();
    let mismatch = run(&Claude, &claude_fake(true), root.path(), &|_| true, "9.9.9").err().unwrap();
    assert_eq!(mismatch.0, "claude-runtime-update: decompressed binary checksum mismatch");
    let invalid = run(&Claude, &claude_fake(false), root.path(), &|_| true, "not-a-version").err().unwrap();
    assert_eq!((invalid.0.as_str(), invalid.1), ("claude-runtime-update: invalid version", 64));
    assert!(!root.path().join("current").exists());
}


#[test]
fn a_reinstall_with_a_different_binary_is_refused() {
    let root = tempfile::tempdir().unwrap();
    run(&Claude, &claude_fake(false), root.path(), &|_| true, "9.9.9").ok().unwrap();
    fs::write(root.path().join("versions/9.9.9/bin/claude"), "tampered").unwrap();
    let refused = run(&Claude, &claude_fake(false), root.path(), &|_| true, "9.9.9").err().unwrap();
    assert_eq!(refused.0, "claude-runtime-update: existing runtime identity differs");
}

// KEEP(bug): GitHub flagged opencode 1.19.5 as latest while 2.2.6 was out, and its process-daemon tags are no program release, so the latest flag would install an old opencode (north#1023)
#[test]
fn the_newest_release_is_the_highest_stable_version_tag_not_githubs_latest_flag_nor_a_sidecar_tag() {
    let listing = r#"[{"tag_name":"process-daemon-v0.5.0","draft":false,"prerelease":false},{"tag_name":"v2.2.7-rc1","draft":false,"prerelease":false},{"tag_name":"v2.3.0","draft":false,"prerelease":true},{"tag_name":"v2.2.6","draft":false,"prerelease":false},{"tag_name":"v2.2.10","draft":false,"prerelease":false},{"tag_name":"v1.19.5","draft":false,"prerelease":false}]"#;
    let network = Fake { payload: "", manifest: listing.to_owned(), calls: RefCell::default() };
    assert_eq!(super::github::newest(&network, "o/r").unwrap(), "2.2.10");
}

// KEEP(bug): installs killed or refused at the rename left 250 MB .stage directories piling up under ~/.local/lib (north#1023)
#[test]
fn an_install_clears_the_stage_directory_an_earlier_killed_install_left_behind() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".stage.dead/bin")).unwrap();
    run(&Claude, &claude_fake(false), root.path(), &|_| true, "9.9.9").ok().unwrap();
    assert!(!root.path().join(".stage.dead").exists() && root.path().join("current").exists());
}
