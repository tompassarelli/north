use std::fs::{self};
use std::path::Path;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::error::{NorthError, NorthResult};
use crate::platform::harness;

use super::paths::{operator_catalog_path, owner_path, repo_root};
use super::{configuration, string_field};

pub(super) fn provenance(owner: &Value) -> NorthResult<Value> {
    let source = owner_path(owner)?;
    Ok(json!({
        "owner": owner,
        "revision": repo_revision(string_field(owner, "repo")?)?,
        "contentDigest": format!("sha256:{}", digest_source(&source)?),
    }))
}

fn digest_source(path: &Path) -> NorthResult<String> {
    let mut digest = Sha256::new();
    digest_path(path, path, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()))
}

fn digest_path(root: &Path, path: &Path, digest: &mut Sha256) -> NorthResult<()> {
    digest.update(
        path.strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .as_bytes(),
    );
    digest.update([0]);
    if path.is_dir() {
        let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            digest_path(root, &entry.path(), digest)?;
        }
    } else if path.is_file() {
        digest.update(fs::read(path)?);
    } else {
        return configuration(format!(
            "owner source has unsupported type: {}",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn catalog_digest() -> NorthResult<String> {
    let paths = [
        repo_root("north")?.join("agent-machinery/catalog.json"),
        operator_catalog_path()?,
    ];
    let mut digest = Sha256::new();
    for path in paths {
        digest.update(fs::read(path)?);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn repo_revision(repo: &str) -> NorthResult<String> {
    harness::revision(&repo_root(repo)?)
        .ok_or_else(|| NorthError::Configuration(format!("cannot resolve revision for {repo}")))
}
