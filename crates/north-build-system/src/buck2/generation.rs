//! The generation of the shared store, which the system configuration writes on a client host and north only reads.
//! Buck2's local state must not outlive a change of it, so each generation has an isolation directory of its own, a
//! daemon and a `buck-out` no other generation used, and a tree's first run under a generation ends the daemons of the
//! others and removes their output directories.
use std::fs::{self, File, TryLockError};
use std::io::ErrorKind;
use std::path::Path;
use std::process::Stdio;

use crate::{Invocation, Shell};

/// The file the system configuration writes: the generation as a decimal integer and one newline, absent while none is set.
pub const FILE: &str = "/etc/buildbarn/store-generation";
pub(crate) const FLAG: &str = "--isolation-dir";
/// Buck2's own isolation directory, where no flag names one.
pub(crate) const DEFAULT: &str = "v2";
const PREFIX: &str = "north-g";
const OUTPUT: &str = "buck-out";
/// Beside the isolation directories in `buck-out`, under a name no generation's directory has, so a retirement leaves it.
const LOCKS: &str = "north-locks";

/// The generation `file` holds, `None` when there is no file; why not, when it cannot be read or holds anything else.
pub(crate) fn read(file: &Path) -> Result<Option<u64>, String> {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read the store generation at {}: {error}", file.display())),
    };
    let digits = text.strip_suffix('\n').filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()));
    digits.and_then(|digits| digits.parse().ok()).map(Some).ok_or_else(|| format!("the store generation at {} holds {text:?}, not a decimal integer and one newline", file.display()))
}

pub(crate) fn isolation(generation: u64) -> String {
    format!("{PREFIX}{generation}")
}

/// An isolation directory of generation `current`'s own beside its main one, named `name`.
pub(crate) fn beside(current: &str, name: &str) -> String {
    format!("{current}-{name}")
}

/// The generation an isolation directory belongs to: `north-gN`, or one beside it, `north-gN-NAME`.
fn of(dir: &str) -> Option<&str> {
    let number = dir.strip_prefix(PREFIX)?.split('-').next()?;
    (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())).then_some(number)
}

/// Ends the daemon of every isolation directory of another generation in the tree at `root` and removes its output directory, printing each it removed.
pub(crate) fn retire(root: &Path, current: &str) {
    let Ok(entries) = fs::read_dir(root.join(OUTPUT)) else { return };
    let others = entries.filter_map(Result::ok).filter_map(|entry| entry.file_name().into_string().ok()).filter(|name| of(name).is_some_and(|generation| Some(generation) != of(current)));
    for other in others {
        let kill = Invocation::new(root, Shell::LandShell, ["buck2", FLAG, &other, "kill"].map(str::to_owned).to_vec());
        let ended = kill.command().stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success());
        let dir = root.join(OUTPUT).join(&other);
        match fs::remove_dir_all(&dir) {
            Ok(()) => eprintln!("north: store generation {current}: {} the Buck2 daemon of {other} and removed {}", if ended { "ended" } else { "found no running" }, dir.display()),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => eprintln!("north: store generation {current}: cannot remove {}: {error}", dir.display()),
        }
    }
}

/// Removes `dir` and all it holds; why not, when it stays.
pub(crate) fn clear(dir: &Path) -> Result<(), String> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot clear {}: {error}", dir.display())),
    }
}

/// The file whose lock gives one Buck2 command at a time the isolation directory `dir` of the tree at `root`.
pub(crate) fn lock_file(root: &Path, dir: &str) -> std::path::PathBuf {
    root.join(OUTPUT).join(LOCKS).join(dir)
}

/// The tree at `root`'s lock on its isolation directory `dir`, taken once any other holder lets it go, held until the file
/// is dropped: two Buck2 clients started together in one directory with no daemon running each start one, and one fails
/// on the state the other is creating (north#1403).
pub(crate) fn hold(root: &Path, dir: &str) -> Result<File, String> {
    let path = lock_file(root, dir);
    let file = path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| File::options().create(true).truncate(false).write(true).open(&path)).map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    match file.try_lock() {
        Ok(()) => return Ok(file),
        Err(TryLockError::WouldBlock) => eprintln!("north: another Buck2 command holds {dir} in {}, so this one waits for it", root.display()),
        Err(TryLockError::Error(error)) => return Err(format!("cannot lock {}: {error}", path.display())),
    }
    file.lock().map_err(|error| format!("cannot lock {}: {error}", path.display()))?;
    Ok(file)
}

/// The lock file of isolation directory `isolation` (Buck2's own when `None`) as a path relative to the tree, for a script that takes the lock in the tree it runs in.
pub(crate) fn lock_word(isolation: Option<&str>) -> String {
    lock_file(Path::new(""), isolation.unwrap_or(DEFAULT)).display().to_string()
}

/// The tree at `root`'s lock on `isolation`, held until the file is dropped; `None` where the side names no isolation directory.
pub(crate) fn held(root: &Path, isolation: Option<&str>) -> Result<Option<File>, String> {
    isolation.map(|dir| hold(root, dir)).transpose()
}
