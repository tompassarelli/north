use std::cell::RefCell;
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256, Sha512};

use super::{Agy, Claude, Failure, Install, Network, Vendor, install};

const CLAUDE_SCRIPT: &str = "#!/bin/sh\necho '9.9.9 (Claude Code)'\n";
const AGY_SCRIPT: &str = "#!/bin/sh\necho '9.9.9'\n";

struct Fake {
    payload: &'static str,
    manifest: String,
    calls: RefCell<Vec<String>>,
}

impl Network for Fake {
    fn text(&self, url: &str, _: bool) -> Result<String, String> {
        self.calls.borrow_mut().push(url.to_owned());
        if url.ends_with("/manifest.json") { Ok(self.manifest.clone()) } else { Ok("9.9.9\n".to_owned()) }
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
}

fn claude_fake(corrupt: bool) -> Fake {
    let sum = if corrupt { "0".repeat(64) } else { format!("{:x}", Sha256::digest(CLAUDE_SCRIPT)) };
    Fake { payload: CLAUDE_SCRIPT, manifest: format!(r#"{{"version":"9.9.9","platforms":{{"linux-x64":{{"checksum":"{sum}"}}}}}}"#), calls: RefCell::default() }
}

fn agy_fake() -> Fake {
    let url = "https://storage.googleapis.com/antigravity-public/antigravity-cli/9.9.9-42/linux-x64/cli_linux_x64.tar.gz";
    let sum = format!("{:x}", Sha512::digest(AGY_SCRIPT));
    Fake { payload: AGY_SCRIPT, manifest: format!(r#"{{"version":"9.9.9","platforms":{{"linux-x64":{{"url":"{url}","sha512":"{sum}"}}}}}}"#), calls: RefCell::default() }
}

fn run(vendor: &dyn Vendor, network: &Fake, root: &Path, probe: &dyn Fn(&Path) -> bool, version: &str) -> Result<String, Failure> {
    install(&Install { vendor, network, root, probe }, version)
}

#[test]
fn a_passing_probe_promotes_the_staged_claude_binary() {
    let root = tempfile::tempdir().unwrap();
    let probed = RefCell::new(Vec::new());
    let probe = |binary: &Path| {
        probed.borrow_mut().push((binary.to_path_buf(), root.path().join("current").exists()));
        true
    };
    let message = run(&Claude, &claude_fake(false), root.path(), &probe, "9.9.9").ok().unwrap();
    assert_eq!(fs::read_link(root.path().join("current")).unwrap(), Path::new("versions/9.9.9"));
    assert!(message.starts_with("claude runtime active: 9.9.9 (Claude Code)\nProvenance: "));
    let probed = probed.into_inner();
    assert_eq!(probed.len(), 1);
    assert!(probed[0].0.starts_with(root.path()) && probed[0].0.to_string_lossy().contains("/.stage.") && probed[0].0.ends_with("bin/claude"));
    assert!(!probed[0].1, "the probe ran before any promotion");
    let provenance = fs::read_to_string(root.path().join("versions/9.9.9/provenance")).unwrap();
    assert!(provenance.contains("version=9.9.9\n") && provenance.contains(&format!("binary_sha256={:x}", Sha256::digest(CLAUDE_SCRIPT))));
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
fn latest_resolves_through_the_vendor_before_installing() {
    let root = tempfile::tempdir().unwrap();
    let network = claude_fake(false);
    run(&Claude, &network, root.path(), &|_| true, "latest").ok().unwrap();
    assert_eq!(network.calls.borrow()[0], "https://downloads.claude.ai/claude-code-releases/latest");
    assert!(fs::read_link(root.path().join("current")).is_ok());
}

#[test]
fn agy_installs_into_a_version_and_build_directory_without_a_probe() {
    let root = tempfile::tempdir().unwrap();
    let message = run(&Agy, &agy_fake(), root.path(), &|_| false, "9.9.9").ok().unwrap();
    assert!(message.starts_with("agy runtime active: 9.9.9\nProvenance: "));
    assert_eq!(fs::read_link(root.path().join("current")).unwrap(), Path::new("versions/9.9.9-42"));
    let provenance = fs::read_to_string(root.path().join("versions/9.9.9-42/provenance")).unwrap();
    assert!(provenance.contains("archive_sha512=") && provenance.contains("source=https://storage.googleapis.com/antigravity-public/antigravity-cli/9.9.9-42/linux-x64/cli_linux_x64.tar.gz\n"));
}

#[test]
fn a_reinstall_with_a_different_binary_is_refused() {
    let root = tempfile::tempdir().unwrap();
    run(&Claude, &claude_fake(false), root.path(), &|_| true, "9.9.9").ok().unwrap();
    fs::write(root.path().join("versions/9.9.9/bin/claude"), "tampered").unwrap();
    let refused = run(&Claude, &claude_fake(false), root.path(), &|_| true, "9.9.9").err().unwrap();
    assert_eq!(refused.0, "claude-runtime-update: existing runtime identity differs");
}
