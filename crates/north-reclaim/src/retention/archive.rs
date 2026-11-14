use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use super::family::Family;
use super::remote::Remote;

type Manifest = BTreeMap<String, String>;

pub(super) type Outcome = Result<(), String>;

fn quoted(names: &[&str]) -> String {
    names.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>().join(" ")
}

fn remote_manifests(remote: &dyn Remote, family: &Family, names: &[&str]) -> io::Result<BTreeMap<String, Manifest>> {
    let list = quoted(names);
    let script = format!("{} && {{ find {list} -type f -print0 | LC_ALL=C sort -z | xargs -0 -r sha256sum && find {list} -type l -printf '-> %l  %p\\n'; }}", family.enter());
    let text = String::from_utf8_lossy(&remote.output(&script)?).into_owned();
    let mut manifests: BTreeMap<String, Manifest> = names.iter().map(|name| ((*name).to_owned(), Manifest::new())).collect();
    for line in text.lines() {
        let (path, hash) = entry(line).ok_or_else(|| io::Error::other(format!("unreadable manifest line: {line}")))?;
        let owner = path.split('/').next().unwrap_or_default();
        manifests.get_mut(owner).ok_or_else(|| io::Error::other(format!("manifest line outside the batch: {line}")))?.insert(path, hash);
    }
    Ok(manifests)
}

fn entry(line: &str) -> Option<(String, String)> {
    match line.strip_prefix("-> ") {
        Some(link) => link.rsplit_once("  ").map(|(target, path)| (path.to_owned(), format!("-> {target}"))),
        None => line.split_once("  ").map(|(hash, path)| (path.to_owned(), hash.to_owned())),
    }
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
    } else if metadata.is_symlink() {
        let relative = path.strip_prefix(root).map_err(io::Error::other)?;
        manifest.insert(relative.to_string_lossy().into_owned(), format!("-> {}", fs::read_link(path)?.to_string_lossy()));
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

fn copy_into(remote: &dyn Remote, family: &Family, names: &[&str], staging: &Path) -> io::Result<()> {
    fs::create_dir_all(staging)?;
    let mut sender = remote.stream(&format!("{} && tar -cf - {}", family.enter(), quoted(names)))?;
    let stdout = sender.stdout.take().ok_or_else(|| io::Error::other("no tar stream"))?;
    let received = Command::new("tar").arg("-xf").arg("-").arg("-C").arg(staging).stdin(Stdio::from(stdout)).status()?;
    let sent = sender.wait()?;
    if received.success() && sent.success() { Ok(()) } else { Err(io::Error::other("tar stream failed")) }
}

pub(super) fn archive(remote: &dyn Remote, family: &Family, names: &[&str], destination: &Path) -> BTreeMap<String, Outcome> {
    let mut outcomes: BTreeMap<String, Outcome> = BTreeMap::new();
    let verified = match verify_copies(remote, family, names, destination, &mut outcomes) {
        Ok(verified) => verified,
        Err(error) => return names.iter().map(|name| ((*name).to_owned(), Err(error.to_string()))).collect(),
    };
    let unchanged: Vec<&str> = match remote_manifests(remote, family, &verified.keys().map(String::as_str).collect::<Vec<_>>()) {
        Ok(again) => verified
            .iter()
            .filter_map(|(name, manifest)| {
                if again.get(name) == Some(manifest) {
                    Some(name.as_str())
                } else {
                    outcomes.insert(name.clone(), Err("changed on the source while it was copied; kept".into()));
                    None
                }
            })
            .collect(),
        Err(error) => {
            outcomes.extend(verified.keys().map(|name| (name.clone(), Err(error.to_string()))));
            Vec::new()
        }
    };
    match delete(remote, family, &unchanged) {
        Ok(deleted) => outcomes.extend(unchanged.iter().map(|name| {
            let outcome = if deleted.contains(*name) { Ok(()) } else { Err("pinned, opened or given a special file since it was judged; kept".into()) };
            ((*name).to_owned(), outcome)
        })),
        Err(error) => outcomes.extend(unchanged.iter().map(|name| ((*name).to_owned(), Err(error.to_string())))),
    }
    outcomes
}

fn delete(remote: &dyn Remote, family: &Family, names: &[&str]) -> io::Result<BTreeSet<String>> {
    if names.is_empty() {
        return Ok(BTreeSet::new());
    }
    let script = format!(
        "{} && root=$(pwd -P) && [ -r /proc/1/cmdline ] && held=$(find /proc/[0-9]*/cwd /proc/[0-9]*/fd/* -maxdepth 0 -lname \"$root/*\" -printf '%l\\n' 2>/dev/null; true) && for name in {}; do if [ ! -e \"$name/PINNED\" ] && [ -z \"$(find \"$name\" ! -type f ! -type d ! -type l -print -quit)\" ] && ! printf '%s\\n' \"$held\" | awk -v p=\"$root/$name\" '$0==p||index($0,p\"/\")==1{{f=1}}END{{exit !f}}'; then chmod -R u+w -- \"$name\" && rm -rf -- \"$name\" && echo \"$name\"; fi || :; done",
        family.enter(),
        quoted(names)
    );
    Ok(String::from_utf8_lossy(&remote.output(&script)?).lines().map(str::to_owned).collect())
}

fn verify_copies(remote: &dyn Remote, family: &Family, names: &[&str], destination: &Path, outcomes: &mut BTreeMap<String, Outcome>) -> io::Result<BTreeMap<String, Manifest>> {
    let expected = remote_manifests(remote, family, names)?;
    let mut verified = BTreeMap::new();
    let mut fresh = Vec::new();
    for (name, manifest) in expected {
        let target = destination.join(&name);
        if !target.exists() {
            fresh.push((name, manifest));
        } else if local_manifest(destination, &name)? == manifest {
            verified.insert(name, manifest);
        } else {
            outcomes.insert(name, Err("archive already holds a different copy".into()));
        }
    }
    if !fresh.is_empty() {
        let staging: PathBuf = destination.join(".incoming").join(std::process::id().to_string());
        if staging.exists() {
            writable(&staging)?;
            fs::remove_dir_all(&staging)?;
        }
        let copied = copy_into(remote, family, &fresh.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), &staging).and_then(|()| writable(&staging));
        if let Err(error) = copied {
            let _ = writable(&staging);
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        for (name, manifest) in fresh {
            match local_manifest(&staging, &name) {
                Ok(local) if local == manifest => {
                    fs::rename(staging.join(&name), destination.join(&name))?;
                    verified.insert(name, manifest);
                }
                Ok(_) => drop(outcomes.insert(name, Err("sha256 of the copy differs from the source".into()))),
                Err(error) => drop(outcomes.insert(name, Err(error.to_string()))),
            }
        }
        fs::remove_dir_all(&staging)?;
    }
    if !verified.is_empty() {
        flush(destination)?;
    }
    Ok(verified)
}

fn writable(staging: &Path) -> io::Result<()> {
    if Command::new("chmod").arg("-R").arg("u+w").arg("--").arg(staging).status()?.success() { Ok(()) } else { Err(io::Error::other("chmod of the staged copy failed")) }
}

fn flush(target: &Path) -> io::Result<()> {
    if Command::new("sync").arg("-f").arg(target).status()?.success() { Ok(()) } else { Err(io::Error::other("sync of the archive copy failed")) }
}
