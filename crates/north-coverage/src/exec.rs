use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

pub const WORK_ENV: &str = "NORTH_COVERAGE_WORK";
pub const TOOLS_ENV: &str = "NORTH_COVERAGE_TOOLS";

pub struct Work {
    pub root: PathBuf,
}

impl Work {
    pub fn raw(&self) -> PathBuf {
        self.root.join("raw")
    }

    pub fn reach(&self) -> PathBuf {
        self.root.join("reach")
    }

    pub fn seed(&self) -> PathBuf {
        self.root.join("seed")
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let Some((binary, rest)) = arguments.split_first() else {
        eprintln!("usage: north coverage exec BINARY [ARGUMENT...]");
        return 2;
    };
    let (Some(work), Some(tools)) = (std::env::var_os(WORK_ENV), std::env::var_os(TOOLS_ENV)) else {
        eprintln!("north coverage exec: {WORK_ENV} and {TOOLS_ENV} must be set by north coverage record");
        return 2;
    };
    let work = Work { root: PathBuf::from(work) };
    let test = exact_test(rest);
    let key = key(binary, test.unwrap_or("--list"));
    let profile = match test {
        Some(_) => work.raw().join(format!("{key}-%4m.profraw")),
        None => work.seed().join(format!("{key}-%4m.profraw")),
    };
    let status = Command::new(binary).args(rest).env("LLVM_PROFILE_FILE", &profile).status();
    let code = status.ok().and_then(|status| status.code()).map_or(1, |code| u8::try_from(code).unwrap_or(1));
    if let Some(test) = test
        && let Err(why) = reduce(&work, Path::new(&tools), &key, binary, test)
    {
        eprintln!("north coverage exec: {why}");
        return code.max(1);
    }
    code
}

fn exact_test(arguments: &[String]) -> Option<&str> {
    arguments.windows(2).find(|pair| pair[0] == "--exact").map(|pair| pair[1].as_str())
}

fn key(binary: &str, test: &str) -> String {
    let digest = Sha256::digest(format!("{binary}\n{test}").as_bytes());
    digest.iter().take(10).map(|byte| format!("{byte:02x}")).collect()
}

fn reduce(work: &Work, tools: &Path, key: &str, binary: &str, test: &str) -> Result<(), String> {
    let raws: Vec<PathBuf> = std::fs::read_dir(work.raw())
        .map_err(|error| format!("cannot list {}: {error}", work.raw().display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with(&format!("{key}-"))))
        .collect();
    let profdata = work.raw().join(format!("{key}.profdata"));
    let mut names = Vec::new();
    if !raws.is_empty() {
        let merged = Command::new(tools.join("llvm-profdata")).arg("merge").arg("-sparse").args(&raws).arg("-o").arg(&profdata).output().map_err(|error| format!("llvm-profdata merge: {error}"))?;
        if !merged.status.success() {
            return Err(format!("llvm-profdata merge failed: {}", String::from_utf8_lossy(&merged.stderr).trim()));
        }
        let shown = Command::new(tools.join("llvm-profdata")).args(["show", "--all-functions"]).arg(&profdata).output().map_err(|error| format!("llvm-profdata show: {error}"))?;
        if !shown.status.success() {
            return Err(format!("llvm-profdata show failed: {}", String::from_utf8_lossy(&shown.stderr).trim()));
        }
        names = executed(&String::from_utf8_lossy(&shown.stdout));
    }
    for path in raws.iter().chain([&profdata]) {
        let _ = std::fs::remove_file(path);
    }
    let mut text = format!("{binary}\t{test}\n");
    names.iter().for_each(|name| {
        text.push_str(name);
        text.push('\n');
    });
    std::fs::write(work.reach().join(key), text).map_err(|error| format!("cannot write the reach of {test}: {error}"))
}

pub fn executed(shown: &str) -> Vec<String> {
    shown.lines().filter(|line| line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':')).map(|line| line[2..line.len() - 1].to_owned()).collect()
}
