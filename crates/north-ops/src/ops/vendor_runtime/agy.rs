use std::fs;
use std::path::Path;
use std::process::Command;

use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha512};

use super::{Failure, Fetched, Network, Vendor, fail, sha256_file, staged_output};

const BASE: &str = "https://storage.googleapis.com/antigravity-public/antigravity-cli";

pub struct Agy;

impl Vendor for Agy {
    fn name(&self) -> &'static str {
        "agy"
    }

    fn latest(&self, network: &dyn Network) -> Result<String, String> {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos());
        network.text(&format!("{BASE}/latest?nocache={nanos}"), true)
    }

    fn fetch(&self, network: &dyn Network, stage: &Path, version: &str) -> Result<Fetched, Failure> {
        let manifest_url = format!("{BASE}/{version}/manifest.json");
        let manifest: Value = network.text(&manifest_url, false).ok().and_then(|text| serde_json::from_str(&text).ok()).ok_or_else(|| fail(self, "manifest unavailable", 1))?;
        let platform = &manifest["platforms"]["linux-x64"];
        let url_shape = Regex::new(&format!(r"^{}-[0-9]+/linux-x64/cli_linux_x64\.tar\.gz$", regex::escape(&format!("{BASE}/{version}")))).map_err(|error| fail(self, &error.to_string(), 1))?;
        let (Some(url), Some(expected)) = (platform["url"].as_str().filter(|url| url_shape.is_match(url)), platform["sha512"].as_str().filter(|sum| Regex::new("^[a-f0-9]{128}$").is_ok_and(|valid| valid.is_match(sum)))) else {
            return Err(fail(self, "manifest does not match the requested version", 1));
        };
        if manifest["version"] != version {
            return Err(fail(self, "manifest does not match the requested version", 1));
        }
        let archive = stage.join("agy.tar.gz");
        let binary = stage.join("bin/agy");
        network.download(url, &archive).map_err(|error| fail(self, &error, 1))?;
        let actual = fs::read(&archive).map(|bytes| format!("{:x}", Sha512::digest(bytes))).map_err(|error| fail(self, &error.to_string(), 1))?;
        if actual != expected {
            return Err(fail(self, "archive checksum mismatch", 1));
        }
        network.untar_member(&archive, "antigravity", &binary).map_err(|error| fail(self, &error, 1))?;
        let binary_sha256 = sha256_file(&binary).map_err(|error| fail(self, &error, 1))?;
        let _ = fs::remove_file(&archive);
        let build = url.strip_prefix(&format!("{BASE}/{version}-")).and_then(|rest| rest.split('/').next()).unwrap_or_default();
        Ok(Fetched { directory: format!("{version}-{build}"), provenance: format!("manifest={manifest_url}\nsource={url}\nversion={version}\narchive_sha512={actual}\nbinary_sha256={binary_sha256}\n"), binary_sha256 })
    }

    fn observe(&self, binary: &Path) -> Option<String> {
        let output = staged_output(Command::new(binary).arg("--version").env("AGY_CLI_DISABLE_AUTO_UPDATE", "1"))?;
        Some(String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
    }

    fn expected(&self, version: &str) -> String {
        version.to_owned()
    }

    fn probe_before_promotion(&self) -> bool {
        false
    }
}
