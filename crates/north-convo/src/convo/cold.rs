use std::fs::{self, File, FileTimes};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::Console;
use super::archive::fsync_path;
use super::env::Env;
use super::failure::Result;

/// Where the off-host push keeps a transcript's cold copy, mirrored from its absolute path on this machine.
pub(super) fn object(cold: &Path, disk: &str) -> PathBuf {
    cold.join(format!("{}.age", disk.strip_prefix('/').unwrap_or(disk)))
}

fn present(logical: &str) -> bool {
    Path::new(logical).exists() || Path::new(&format!("{logical}.zst")).exists()
}

fn decrypt(env: &Env, from: &Path, to: &Path) -> std::io::Result<()> {
    let identity = env.identity.as_deref().ok_or_else(|| std::io::Error::other("CONVO_AGE_IDENTITY is unset, so no key can open the cold copy"))?;
    let out = Command::new("age").args(["-d", "-i"]).arg(identity).arg("-o").arg(to).arg(from).output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(String::from_utf8_lossy(&out.stderr).trim().to_owned()))
    }
}

fn verified(env: &Env, path: &Path) -> std::io::Result<()> {
    if path.extension().is_none_or(|extension| extension != "zst") {
        return Ok(());
    }
    let out = Command::new(&env.zstd).args(["-dcq"]).arg(path).stdout(Stdio::null()).stderr(Stdio::piped()).output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("the cold copy is not a readable archive: {}", String::from_utf8_lossy(&out.stderr).trim())))
    }
}

fn restored(env: &Env, from: &Path, disk: &str) -> std::io::Result<()> {
    let tmp = PathBuf::from(format!("{disk}.tmp{}", std::process::id()));
    let done = decrypt(env, from, &tmp).and_then(|()| verified(env, &tmp)).and_then(|()| {
        fsync_path(&tmp)?;
        fs::rename(&tmp, disk)?;
        fsync_path(Path::new(disk).parent().unwrap_or(Path::new(".")))
    });
    if done.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    done?;
    if let Ok(modified) = fs::metadata(from).and_then(|meta| meta.modified()) {
        let _ = File::options().write(true).open(disk).and_then(|file| file.set_times(FileTimes::new().set_modified(modified)));
    }
    Ok(())
}

fn fetch(env: &Env, cold: &Path, logical: &str) -> Result<Option<String>> {
    for disk in [format!("{logical}.zst"), logical.to_owned()] {
        let from = object(cold, &disk);
        if !from.is_file() {
            continue;
        }
        restored(env, &from, &disk)?;
        return Ok(Some(disk));
    }
    Ok(None)
}

/// Brings back the named transcripts this laptop has evicted, when the cold copy holds them.
pub(super) fn recall<'a>(env: &Env, io: &mut Console, paths: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let Some(cold) = env.cold.as_deref() else { return Ok(()) };
    let mut seen: Vec<&str> = Vec::new();
    for path in paths {
        if seen.contains(&path) || present(path) {
            continue;
        }
        seen.push(path);
        match fetch(env, cold, path) {
            Ok(Some(disk)) => writeln!(io.err, "convo: fetched {disk} from its cold copy")?,
            Ok(None) => {}
            Err(failure) => writeln!(io.err, "convo: could not fetch {path} from its cold copy: {failure}")?,
        }
    }
    Ok(())
}
