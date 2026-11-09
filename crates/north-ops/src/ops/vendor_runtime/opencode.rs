use std::path::Path;
use std::process::Command;

use super::github::{self, Release};
use super::{Failure, Fetched, Network, Vendor, staged_output};

const RELEASE: Release = Release { repo: "anomalyco/opencode", asset: "opencode-linux-x64.tar.gz", strip: 0 };

pub struct Opencode;

impl Vendor for Opencode {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn latest(&self, network: &dyn Network) -> Result<String, String> {
        github::newest(network, RELEASE.repo)
    }

    fn fetch(&self, network: &dyn Network, stage: &Path, version: &str) -> Result<Fetched, Failure> {
        RELEASE.fetch(self, network, stage, version)
    }

    fn observe(&self, binary: &Path) -> Option<String> {
        let output = staged_output(Command::new(binary).arg("--version").env("OPENCODE_DISABLE_AUTOUPDATE", "true"))?;
        Some(String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
    }

    fn expected(&self, version: &str) -> String {
        format!("v{version}")
    }

    fn probe_before_promotion(&self) -> bool {
        false
    }
}
