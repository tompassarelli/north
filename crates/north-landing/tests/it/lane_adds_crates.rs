use std::fs;
use std::path::PathBuf;
use std::process::Command;

use north_base::foundation::shim::write_executable;
use north_landing::land::checks::unfit_host;
use north_landing::land::slots::deps::fetch_crates_recorded;

const DEPBOX: &str = "#!/bin/sh\necho \"depbox $*\" >>\"$(dirname \"$0\")/fetches.log\"\n[ ! -e \"$(dirname \"$0\")/unfetchable\" ] || { printf 'error: failed to download `aes v0.8.4`\\n\\nCaused by:\\n  HTTP 502\\n' >&2; exit 101; }\n";

struct Host {
    _scratch: tempfile::TempDir,
    checkout: PathBuf,
    fetched: PathBuf,
}

impl Host {
    fn with_lane_adding_a_crate(unfetchable: bool) -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let checkout = scratch.path().join("lane");
        fs::create_dir_all(checkout.join("bin")).unwrap();
        write_executable(&checkout.join("bin/depbox"), DEPBOX);
        if unfetchable {
            fs::write(checkout.join("bin/unfetchable"), "").unwrap();
        }
        fs::write(checkout.join("Cargo.lock"), "[[package]]\nname = \"aes\"\nversion = \"0.8.4\"\n").unwrap();
        for arguments in [&["init", "-q", "."][..], &["add", "."], &["-c", "user.name=n", "-c", "user.email=n@n", "commit", "-q", "-m", "lane"]] {
            assert!(Command::new("git").args(arguments).current_dir(&checkout).status().unwrap().success());
        }
        Self { fetched: scratch.path().join("cargo-fetched"), _scratch: scratch, checkout }
    }

    fn fetches(&self) -> String {
        fs::read_to_string(self.checkout.join("bin/fetches.log")).unwrap_or_default()
    }
}

#[test]
fn a_lane_that_adds_a_crate_the_host_lacks_is_fetched_and_checked_and_a_crate_that_cannot_be_fetched_refuses_that_lane_alone() {
    let host = Host::with_lane_adding_a_crate(false);
    fetch_crates_recorded(&host.checkout, &host.fetched).unwrap();
    assert_eq!(host.fetches(), "depbox fetch -- cargo fetch --locked\n");
    fetch_crates_recorded(&host.checkout, &host.fetched).unwrap();
    assert_eq!(host.fetches(), "depbox fetch -- cargo fetch --locked\n", "the second check of the same lock fetches nothing");

    let unfetchable = Host::with_lane_adding_a_crate(true);
    let refused = fetch_crates_recorded(&unfetchable.checkout, &unfetchable.fetched).unwrap_err();
    assert!(refused.contains("cargo fetch --locked failed"), "{refused}");
    assert_eq!(unfit_host(&refused), None, "a fetch that failed online refuses this lane; it is not a wait for the host");
    assert!(!unfetchable.fetched.exists(), "a failed fetch records nothing, so the next check tries again");
}
