use std::fs;
use std::path::Path;

use regex::Regex;
use serde_json::Value;

use super::{Failure, Fetched, Network, Vendor, fail, sha256_file};

pub struct Release {
    pub repo: &'static str,
    pub asset: &'static str,
    pub strip: u32,
}

fn tag_version(tag: &str) -> Option<(u64, u64, u64)> {
    let parts = Regex::new(r"^v([0-9]+)\.([0-9]+)\.([0-9]+)$").ok()?.captures(tag)?;
    Some((parts[1].parse().ok()?, parts[2].parse().ok()?, parts[3].parse().ok()?))
}

pub fn newest(network: &dyn Network, repo: &str) -> Result<String, String> {
    let listing: Value = serde_json::from_str(&network.text(&format!("https://api.github.com/repos/{repo}/releases?per_page=50"), true)?).map_err(|error| error.to_string())?;
    let newest = listing.as_array().into_iter().flatten().filter(|release| release["draft"] == false && release["prerelease"] == false).filter_map(|release| release["tag_name"].as_str()).filter_map(|tag| tag_version(tag).map(|key| (key, tag))).max();
    newest.map(|(_, tag)| tag.trim_start_matches('v').to_owned()).ok_or_else(|| format!("{repo} lists no stable vMAJOR.MINOR.PATCH release"))
}

impl Release {
    pub fn fetch(&self, vendor: &dyn Vendor, network: &dyn Network, stage: &Path, version: &str) -> Result<Fetched, Failure> {
        let api = format!("https://api.github.com/repos/{}/releases/tags/v{version}", self.repo);
        let release: Value = network.text(&api, false).ok().and_then(|text| serde_json::from_str(&text).ok()).ok_or_else(|| fail(vendor, "release unavailable", 1))?;
        let prefix = format!("https://github.com/{}/releases/download/v{version}/", self.repo);
        let asset = release["assets"].as_array().into_iter().flatten().find(|asset| asset["name"] == self.asset);
        let (Some(url), Some(expected)) = (
            asset.and_then(|asset| asset["browser_download_url"].as_str()).filter(|url| *url == format!("{prefix}{}", self.asset)),
            asset.and_then(|asset| asset["digest"].as_str()).and_then(|digest| digest.strip_prefix("sha256:")).filter(|sum| Regex::new("^[a-f0-9]{64}$").is_ok_and(|valid| valid.is_match(sum))),
        ) else {
            return Err(fail(vendor, "release does not carry a checksummed asset for the requested version", 1));
        };
        let archive = stage.join("archive.tar.gz");
        network.download(url, &archive).map_err(|error| fail(vendor, &error, 1))?;
        let actual = sha256_file(&archive).map_err(|error| fail(vendor, &error, 1))?;
        if actual != expected {
            return Err(fail(vendor, "archive checksum mismatch", 1));
        }
        network.untar_into(&archive, &stage.join("bin"), self.strip).map_err(|error| fail(vendor, &error, 1))?;
        let _ = fs::remove_file(&archive);
        let binary_sha256 = sha256_file(&stage.join("bin").join(vendor.name())).map_err(|error| fail(vendor, &error, 1))?;
        Ok(Fetched { directory: version.to_owned(), provenance: format!("source={url}\nversion={version}\narchive_sha256={actual}\nbinary_sha256={binary_sha256}\n"), binary_sha256 })
    }
}
