mod agy;
mod claude;
#[cfg(test)]
mod tests;

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;
use rustix::fs::{FlockOperation, flock};
use sha2::{Digest, Sha256};

pub use agy::Agy;
pub use claude::Claude;

pub struct Failure(pub String, pub u8);

pub trait Network {
    fn text(&self, url: &str, fresh: bool) -> Result<String, String>;
    fn download(&self, url: &str, destination: &Path) -> Result<(), String>;
    fn unzstd(&self, input: &Path, output: &Path) -> Result<(), String>;
    fn untar_member(&self, archive: &Path, member: &str, output: &Path) -> Result<(), String>;
}

pub struct Fetched {
    pub directory: String,
    pub binary_sha256: String,
    pub provenance: String,
}

pub trait Vendor {
    fn name(&self) -> &'static str;
    fn latest(&self, network: &dyn Network) -> Result<String, String>;
    fn fetch(&self, network: &dyn Network, stage: &Path, version: &str) -> Result<Fetched, Failure>;
    fn observe(&self, binary: &Path) -> Option<String>;
    fn expected(&self, version: &str) -> String;
    fn probe_before_promotion(&self) -> bool;
}

struct Curl;

fn run_ok(command: &mut Command) -> Result<std::process::Output, String> {
    command.output().map_err(|error| error.to_string()).and_then(|output| if output.status.success() { Ok(output) } else { Err(format!("{:?} exited {}", command.get_program(), output.status)) })
}

impl Network for Curl {
    fn text(&self, url: &str, fresh: bool) -> Result<String, String> {
        let mut curl = Command::new("curl");
        curl.arg("-fsSL");
        if fresh {
            curl.args(["-H", "Cache-Control: no-cache"]);
        }
        run_ok(curl.arg(url)).map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn download(&self, url: &str, destination: &Path) -> Result<(), String> {
        run_ok(Command::new("curl").args(["-fsSL", url, "-o"]).arg(destination)).map(|_| ())
    }

    fn unzstd(&self, input: &Path, output: &Path) -> Result<(), String> {
        run_ok(Command::new("zstd").arg("-dq").arg(input).arg("-o").arg(output)).map(|_| ())
    }

    fn untar_member(&self, archive: &Path, member: &str, output: &Path) -> Result<(), String> {
        let extracted = run_ok(Command::new("tar").arg("-xOzf").arg(archive).arg(member))?;
        fs::write(output, extracted.stdout).map_err(|error| error.to_string())
    }
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    fs::read(path).map(|bytes| format!("{:x}", Sha256::digest(bytes))).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn fail(vendor: &dyn Vendor, message: &str, code: u8) -> Failure {
    Failure(format!("{}-runtime-update: {message}", vendor.name()), code)
}

pub struct Install<'a> {
    pub vendor: &'a dyn Vendor,
    pub network: &'a dyn Network,
    pub root: &'a Path,
    pub probe: &'a dyn Fn(&Path) -> bool,
}

pub fn install(request: &Install, requested: &str) -> Result<String, Failure> {
    let vendor = request.vendor;
    let version = if requested == "latest" { vendor.latest(request.network).map_err(|error| fail(vendor, &error, 1))?.trim().to_owned() } else { requested.to_owned() };
    if !Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+([.-][a-zA-Z0-9.]+)?$").is_ok_and(|valid| valid.is_match(&version)) {
        return Err(fail(vendor, "invalid version", 64));
    }
    if std::env::consts::OS != "linux" || std::env::consts::ARCH != "x86_64" {
        return Err(fail(vendor, "unsupported platform", 64));
    }
    let io = |error: std::io::Error| fail(vendor, &error.to_string(), 1);
    fs::create_dir_all(request.root.join("versions")).map_err(io)?;
    let lock = fs::OpenOptions::new().create(true).append(true).open(request.root.join("update.lock")).map_err(io)?;
    flock(&lock, FlockOperation::LockExclusive).map_err(|error| fail(vendor, &error.to_string(), 1))?;
    let stage = tempfile::Builder::new().prefix(".stage.").tempdir_in(request.root).map_err(io)?;
    fs::create_dir(stage.path().join("bin")).map_err(io)?;
    let fetched = vendor.fetch(request.network, stage.path(), &version)?;
    let binary = stage.path().join("bin").join(vendor.name());
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).map_err(io)?;
    let observed = vendor.observe(&binary).unwrap_or_default();
    if observed != vendor.expected(&version) {
        return Err(fail(vendor, "binary version mismatch", 1));
    }
    if vendor.probe_before_promotion() && !(request.probe)(&binary) {
        return Err(fail(vendor, "contract probe failed; refusing promotion", 1));
    }
    fs::write(stage.path().join("provenance"), &fetched.provenance).map_err(io)?;
    let destination = request.root.join("versions").join(&fetched.directory);
    if destination.exists() {
        let installed = sha256_file(&destination.join("bin").join(vendor.name())).map_err(|error| fail(vendor, &error, 1))?;
        if installed != fetched.binary_sha256 {
            return Err(fail(vendor, "existing runtime identity differs", 1));
        }
    } else {
        fs::rename(stage.keep(), &destination).map_err(io)?;
    }
    let next = request.root.join(format!(".current.{}", std::process::id()));
    let _ = fs::remove_file(&next);
    symlink(Path::new("versions").join(&fetched.directory), &next).map_err(io)?;
    fs::rename(&next, request.root.join("current")).map_err(io)?;
    Ok(format!("{} runtime active: {observed}\nProvenance: {}/provenance", vendor.name(), destination.display()))
}

fn root_for(vendor: &dyn Vendor) -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/lib").join(vendor.name())
}

fn probe(binary: &Path) -> bool {
    std::env::current_exe().is_ok_and(|north| Command::new(north).arg("contract-probe").arg("--runtime").arg(binary).status().is_ok_and(|status| status.success()))
}

fn run_vendor(vendor: &dyn Vendor, arguments: &[String]) -> u8 {
    if arguments.len() > 1 {
        eprintln!("usage: north {}-runtime-update [version|latest]", vendor.name());
        return 64;
    }
    let request = Install { vendor, network: &Curl, root: &root_for(vendor), probe: &probe };
    match install(&request, arguments.first().map_or("latest", String::as_str)) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(Failure(message, code)) => {
            eprintln!("{message}");
            code
        }
    }
}

pub fn run_claude(arguments: &[String]) -> u8 {
    run_vendor(&Claude, arguments)
}

pub fn run_agy(arguments: &[String]) -> u8 {
    run_vendor(&Agy, arguments)
}
