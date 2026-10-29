use std::collections::BTreeSet;
use std::io;
use std::time::Duration;

use super::family::{safe_name, Family};
use super::remote::Remote;

const DERIVED: &str = "derived";
const UNPINNED: [&str; 3] = ["views", "locks", DERIVED];
const PIN_PATHS: &str = "build-inputs ts/stage-thumbnails.json";
const MAIN_ENDS: &str = "--main-ends--";

pub(super) struct Layout {
    pub(super) store: String,
    pub(super) checkouts: String,
}

impl Layout {
    pub(super) fn gex45() -> Self {
        Self { store: "\"$HOME/.local/share/smashcraft-build-inputs/store\"".into(), checkouts: "\"$HOME/code/smashcraft\"".into() }
    }
}

pub(super) fn families(remote: &dyn Remote, layout: &Layout, max_age: Duration) -> io::Result<Vec<Family>> {
    let listing = format!("cd {} 2>/dev/null || exit 0; find . -mindepth 1 -maxdepth 1 -type d -printf '%P\\n'; [ ! -d {DERIVED} ] || find {DERIVED} -mindepth 1 -maxdepth 1 -type d -printf '{DERIVED}/%P\\n'", layout.store);
    let listing = String::from_utf8_lossy(&remote.output(&listing)?).into_owned();
    let mut families = Vec::new();
    let mut pins = None;
    for dir in listing.lines() {
        let mut family = Family::new(&format!("build-inputs/{dir}"), &layout.store, dir, max_age);
        match dir.split_once('/') {
            Some((DERIVED, step)) if safe_name(step) => family.files_are_entries = true,
            None if safe_name(dir) && !UNPINNED.contains(&dir) => {
                if pins.is_none() {
                    pins = Some(read_pins(remote, layout)?);
                }
                family.pins.clone_from(pins.as_ref().expect("pins were read"));
            }
            _ => continue,
        }
        families.push(family);
    }
    Ok(families)
}

fn read_pins(remote: &dyn Remote, layout: &Layout) -> io::Result<BTreeSet<String>> {
    let script = format!(
        "set -e; cd {}; git -C main fetch -q --prune origin; git -C main grep -h -I -e '' origin/main -- {PIN_PATHS}; echo {MAIN_ENDS}; refs=$(git -C main for-each-ref --format='%(refname)' refs/remotes/origin/); for ref in $refs; do git -C main grep -h -I -e '' \"$ref\" -- {PIN_PATHS} || [ $? -eq 1 ]; done; for lane in */ worktrees/*/; do for path in {PIN_PATHS}; do [ ! -e \"$lane$path\" ] || find \"$lane$path\" -type f -exec awk 1 {{}} +; done; done",
        layout.checkouts
    );
    let text = String::from_utf8_lossy(&remote.output(&script)?).into_owned();
    let (main, others) = text.split_once(&format!("{MAIN_ENDS}\n")).ok_or_else(|| io::Error::other("pin read did not finish"))?;
    let on_main = hashes(main);
    if on_main.is_empty() {
        return Err(io::Error::other("smashcraft origin/main names no build-input pin; no store entry is archived"));
    }
    Ok(on_main.into_iter().chain(hashes(others)).collect())
}

fn hashes(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_ascii_alphanumeric()).filter(|word| word.len() == 64 && word.bytes().all(|byte| byte.is_ascii_hexdigit())).map(str::to_owned).collect()
}
