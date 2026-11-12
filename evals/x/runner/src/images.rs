use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, PoisonError};

use crate::error::{Result, RunError};

pub struct Images {
    path: PathBuf,
    first_pulled: Mutex<BTreeMap<String, String>>,
}

impl Images {
    pub fn open(path: PathBuf) -> Result<Self> {
        let first_pulled = match fs::read_to_string(&path) {
            Ok(text) => text
                .lines()
                .filter_map(|line| line.split_once('\t'))
                .map(|(image, digest)| (image.to_owned(), digest.to_owned()))
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            first_pulled: Mutex::new(first_pulled),
        })
    }

    pub fn check(&self, image: &str) -> Result<Option<String>> {
        let found = local_digest(image)?;
        let mut first_pulled = self
            .first_pulled
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match (found, first_pulled.get(image).cloned()) {
            (None, recorded) => Ok(recorded),
            (Some(found), Some(recorded)) if found != recorded => Err(RunError::Digest {
                image: image.to_owned(),
                recorded,
                found,
            }),
            (Some(_), Some(recorded)) => Ok(Some(recorded)),
            (Some(found), None) => {
                first_pulled.insert(image.to_owned(), found.clone());
                self.save(&first_pulled)?;
                Ok(Some(found))
            }
        }
    }

    fn save(&self, first_pulled: &BTreeMap<String, String>) -> Result<()> {
        let text: String = first_pulled
            .iter()
            .map(|(image, digest)| format!("{image}\t{digest}\n"))
            .collect();
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("tmp");
        fs::write(&temporary, text)?;
        fs::rename(&temporary, &self.path)?;
        Ok(())
    }
}

pub fn size_bytes(image: &str) -> Option<u64> {
    let output = Command::new("podman")
        .args(["image", "inspect", "--format", "{{.Size}}", image])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn local_digest(image: &str) -> Result<Option<String>> {
    let output = Command::new("podman")
        .args(["image", "inspect", "--format", "{{.Digest}}", image])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let digest = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!digest.is_empty()).then_some(digest))
}
