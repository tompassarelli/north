use std::fs;
use std::path::Path;

const PRIORS: &str = "host.priors";

/// A scope an earlier host of the session left behind, and the pid of that host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prior {
    pub unit: String,
    pub host: u32,
}

impl Prior {
    pub fn all(dir: &Path) -> Vec<Prior> {
        let text = fs::read_to_string(dir.join(PRIORS)).unwrap_or_default();
        text.lines().filter_map(|line| line.split_once(' ')).filter_map(|(unit, host)| Some(Prior { unit: unit.to_owned(), host: host.parse().ok()? })).collect()
    }

    pub fn remember(&self, dir: &Path) -> std::io::Result<()> {
        let mut text = fs::read_to_string(dir.join(PRIORS)).unwrap_or_default();
        text.push_str(&format!("{} {}\n", self.unit, self.host));
        fs::write(dir.join(PRIORS), text)
    }

    pub fn forget_all(dir: &Path) {
        let _ = fs::remove_file(dir.join(PRIORS));
    }
}
