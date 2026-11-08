use std::fs;
use std::path::{Path, PathBuf};

use super::{NO_RECORDING, RUNNER_KEY};

const STAGE: &str = "north-reuse";
const NEXTEST_RUN: &str = "cargo nextest run";

pub struct Recording {
    pub records: tempfile::TempDir,
    pub command: String,
}

pub fn inside_root(root: &Path) -> Option<PathBuf> {
    root.join("bin/depbox").is_file().then_some(())?;
    let output = std::process::Command::new("git").arg("-C").arg(root).args(["rev-parse", "--git-common-dir"]).output().ok()?;
    let common = root.join(String::from_utf8_lossy(&output.stdout).trim()).canonicalize().ok()?;
    let project = common.parent()?.parent()?.file_name()?;
    let home = PathBuf::from(std::env::var_os("HOME")?).canonicalize().ok()?;
    Some(home.join("code").join(project).join("sandbox"))
}

fn toml_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn seen(path: &Path, root: &Path, inside: &Path) -> PathBuf {
    path.strip_prefix(root).map_or_else(|_| path.to_owned(), |relative| inside.join(relative))
}

pub fn stage(root: &Path, inside: Option<&Path>, target: &Path, command: &str) -> Option<Recording> {
    let stage = target.join(STAGE);
    fs::create_dir_all(&stage).ok()?;
    let north = stage.join("north");
    let current = std::env::current_exe().ok()?;
    let same = fs::metadata(&north).ok().zip(fs::metadata(&current).ok()).is_some_and(|(staged, own)| staged.len() == own.len() && staged.modified().ok() >= own.modified().ok());
    if !same {
        let pending = stage.join(format!("north.{}", std::process::id()));
        fs::copy(&current, &pending).ok()?;
        fs::rename(&pending, &north).ok()?;
    }
    let records = tempfile::Builder::new().prefix("records-").tempdir_in(&stage).ok()?;
    let inside = inside.unwrap_or(root);
    let words = [seen(&north, root, inside).display().to_string(), "land".into(), "sandbox".into(), "exec".into(), "--record".into(), seen(records.path(), root, inside).display().to_string()]
        .into_iter()
        .chain(NO_RECORDING.iter().flat_map(|name| ["--except".to_owned(), (*name).to_owned()]))
        .chain(["--tree".into(), inside.display().to_string(), "--".into()])
        .map(|word| toml_string(&word))
        .collect::<Vec<_>>();
    let option = format!("--config '{RUNNER_KEY}=[{}]'", words.join(","));
    let command = command.replacen(NEXTEST_RUN, &format!("{NEXTEST_RUN} {option}"), 1);
    Some(Recording { records, command })
}
