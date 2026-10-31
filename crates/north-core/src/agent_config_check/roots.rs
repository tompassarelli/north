use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub(super) struct Roots {
    pub(super) home: PathBuf,
    named: BTreeMap<String, PathBuf>,
}

impl Roots {
    pub(super) fn from_env() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let raw = std::env::var("NORTH_REPO_ROOTS").ok().filter(|text| !text.is_empty());
        Self::parse(home, raw.as_deref())
    }

    pub(super) fn parse(home: PathBuf, raw: Option<&str>) -> Self {
        let named = raw
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|value| value.as_object().cloned())
            .map(|object| {
                object
                    .into_iter()
                    .filter_map(|(name, path)| path.as_str().filter(|path| !path.is_empty()).map(|path| (name, PathBuf::from(path))))
                    .collect()
            })
            .unwrap_or_default();
        Self { home, named }
    }

    pub(super) fn named(&self, name: &str) -> Option<&Path> {
        self.named.get(name).map(PathBuf::as_path)
    }

    pub(super) fn checkout(&self, name: &str) -> PathBuf {
        self.named(name).map_or_else(|| self.home.join("code").join(name).join("main"), Path::to_path_buf)
    }
}
