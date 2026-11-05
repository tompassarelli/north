use std::fs;
use std::path::Path;
use std::process::Command;

use regex::Regex;
use serde_json::Value;

use super::{Failure, Fetched, Network, Vendor, fail, sha256_file};

const BASE: &str = "https://downloads.claude.ai/claude-code-releases";

pub struct Claude;

impl Vendor for Claude {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn latest(&self, network: &dyn Network) -> Result<String, String> {
        network.text(&format!("{BASE}/latest"), false)
    }

    fn fetch(&self, network: &dyn Network, stage: &Path, version: &str) -> Result<Fetched, Failure> {
        let manifest: Value = network.text(&format!("{BASE}/{version}/manifest.json"), false).ok().and_then(|text| serde_json::from_str(&text).ok()).ok_or_else(|| fail(self, "manifest unavailable", 1))?;
        let expected = manifest["platforms"]["linux-x64"]["checksum"].as_str().filter(|sum| manifest["version"] == version && Regex::new("^[a-f0-9]{64}$").is_ok_and(|valid| valid.is_match(sum))).ok_or_else(|| fail(self, "manifest does not match the requested version", 1))?;
        let compressed = stage.join("claude.zst");
        let binary = stage.join("bin/claude");
        let source = format!("{BASE}/{version}/linux-x64/claude.zst");
        network.download(&source, &compressed).and_then(|()| network.unzstd(&compressed, &binary)).map_err(|error| fail(self, &error, 1))?;
        let _ = fs::remove_file(&compressed);
        let actual = sha256_file(&binary).map_err(|error| fail(self, &error, 1))?;
        if actual != expected {
            return Err(fail(self, "decompressed binary checksum mismatch", 1));
        }
        Ok(Fetched { directory: version.to_owned(), provenance: format!("source={source}\nversion={version}\nbinary_sha256={actual}\n"), binary_sha256: actual })
    }

    fn observe(&self, binary: &Path) -> Option<String> {
        let output = Command::new(binary).arg("--version").env("DISABLE_AUTOUPDATER", "1").env("DISABLE_INSTALLATION_CHECKS", "1").output().ok().filter(|output| output.status.success())?;
        Some(String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
    }

    fn expected(&self, version: &str) -> String {
        format!("{version} (Claude Code)")
    }

    fn probe_before_promotion(&self) -> bool {
        true
    }
}
