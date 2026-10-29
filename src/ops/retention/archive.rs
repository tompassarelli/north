use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use super::family::Family;
use super::remote::Remote;

type Manifest = BTreeMap<String, String>;

fn remote_manifest(remote: &dyn Remote, family: &Family, name: &str) -> io::Result<Manifest> {
    let script = format!("{} && find '{name}' -type f -print0 | LC_ALL=C sort -z | xargs -0 -r sha256sum", family.enter());
    let text = String::from_utf8_lossy(&remote.output(&script)?).into_owned();
    text.lines()
        .map(|line| line.split_once("  ").map(|(hash, path)| (path.to_owned(), hash.to_owned())).ok_or_else(|| io::Error::other(format!("unreadable checksum line: {line}"))))
        .collect()
}

fn local_manifest(root: &Path, name: &str) -> io::Result<Manifest> {
    let mut manifest = Manifest::new();
    walk(root, &root.join(name), &mut manifest)?;
    Ok(manifest)
}

fn walk(root: &Path, path: &Path, manifest: &mut Manifest) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            walk(root, &entry?.path(), manifest)?;
        }
    } else if metadata.is_file() {
        let mut file = fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 1 << 20];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let relative = path.strip_prefix(root).map_err(io::Error::other)?;
        manifest.insert(relative.to_string_lossy().into_owned(), format!("{:x}", hasher.finalize()));
    }
    Ok(())
}

fn copy_into(remote: &dyn Remote, family: &Family, name: &str, staging: &Path) -> io::Result<()> {
    fs::create_dir_all(staging)?;
    let mut sender = remote.stream(&format!("{} && tar -cf - '{name}'", family.enter()))?;
    let stdout = sender.stdout.take().ok_or_else(|| io::Error::other("no tar stream"))?;
    let received = Command::new("tar").arg("-xf").arg("-").arg("-C").arg(staging).stdin(Stdio::from(stdout)).status()?;
    let sent = sender.wait()?;
    if received.success() && sent.success() { Ok(()) } else { Err(io::Error::other("tar stream failed")) }
}

pub(super) fn archive(remote: &dyn Remote, family: &Family, name: &str, destination: &Path) -> Result<(), String> {
    let verified = verify_copy(remote, family, name, destination).map_err(|error| format!("{name}: {error}"))?;
    let again = remote_manifest(remote, family, name).map_err(|error| format!("{name}: {error}"))?;
    if again != verified {
        return Err(format!("{name}: changed on the source while it was copied; kept"));
    }
    let script = format!(
        "{} && root=$(pwd -P) && [ -r /proc/1/cmdline ] && [ ! -e '{name}/PINNED' ] && [ -z \"$(find '{name}' ! -type f ! -type d -print -quit)\" ] && [ -z \"$(find /proc/[0-9]*/cwd /proc/[0-9]*/fd/* -maxdepth 0 \\( -lname \"$root/{name}\" -o -lname \"$root/{name}/*\" \\) -print -quit 2>/dev/null)\" ] && rm -rf -- '{name}'",
        family.enter()
    );
    remote.output(&script).map(drop).map_err(|error| format!("{name}: {error}"))
}

fn verify_copy(remote: &dyn Remote, family: &Family, name: &str, destination: &Path) -> io::Result<Manifest> {
    let expected = remote_manifest(remote, family, name)?;
    let target = destination.join(name);
    if target.exists() {
        if local_manifest(destination, name)? != expected {
            return Err(io::Error::other("archive already holds a different copy"));
        }
        flush(&target)?;
        return Ok(expected);
    }
    let staging: PathBuf = destination.join(".incoming").join(format!("{name}.{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    let copied = copy_into(remote, family, name, &staging).and_then(|()| local_manifest(&staging, name));
    match copied {
        Ok(local) if local == expected => {
            fs::rename(staging.join(name), &target)?;
            fs::remove_dir_all(&staging)?;
            flush(&target)?;
            Ok(expected)
        }
        Ok(_) => {
            fs::remove_dir_all(&staging)?;
            Err(io::Error::other("sha256 of the copy differs from the source"))
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            Err(error)
        }
    }
}

fn flush(target: &Path) -> io::Result<()> {
    if Command::new("sync").arg("-f").arg(target).status()?.success() { Ok(()) } else { Err(io::Error::other("sync of the archive copy failed")) }
}
