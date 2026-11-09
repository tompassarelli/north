use std::path::Path;
use std::process::Command;

use super::github::{self, Release};
use super::{Failure, Fetched, Network, Vendor, staged_output};

const RELEASE: Release = Release { repo: "earendil-works/pi", asset: "pi-linux-x64.tar.gz", strip: 1 };

pub struct Pi;

impl Vendor for Pi {
    fn name(&self) -> &'static str {
        "pi"
    }

    fn latest(&self, network: &dyn Network) -> Result<String, String> {
        github::newest(network, RELEASE.repo)
    }

    fn fetch(&self, network: &dyn Network, stage: &Path, version: &str) -> Result<Fetched, Failure> {
        RELEASE.fetch(self, network, stage, version)
    }

    fn observe(&self, binary: &Path) -> Option<String> {
        let output = staged_output(Command::new(binary).arg("--version").env("PI_OFFLINE", "1").env("PI_SKIP_VERSION_CHECK", "1").env("PI_TELEMETRY", "0"))?;
        Some(String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
    }

    fn expected(&self, version: &str) -> String {
        version.to_owned()
    }

    fn probe_before_promotion(&self) -> bool {
        false
    }
}
