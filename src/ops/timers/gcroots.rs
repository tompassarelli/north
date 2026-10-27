use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{env_or, home};

#[derive(Default)]
struct Tally {
    rooted: u32,
    unresolved: u32,
    unresolved_current: u32,
}

pub(crate) fn run(_arguments: &[String]) -> u8 {
    let versions = env_or("CODEX_RUNTIME_VERSIONS", || home().join(".local/lib/codex/versions"));
    if !versions.is_dir() {
        eprintln!("codex-runtime-gcroots: no runtimes at {}", versions.display());
        return 0;
    }
    for tool in ["patchelf", "nix-store"] {
        if !super::on_path(tool) {
            eprintln!("codex-runtime-gcroots: {tool} is required");
            return 127;
        }
    }
    let versions_text = versions.to_string_lossy().into_owned();
    let base = versions_text.strip_suffix("/versions").unwrap_or(&versions_text);
    let current = fs::canonicalize(Path::new(base).join("current/bin/codex")).ok();
    let mut tally = Tally::default();
    let mut runtimes: Vec<PathBuf> = fs::read_dir(&versions).into_iter().flatten().flatten().map(|entry| entry.path().join("bin/codex")).collect();
    runtimes.sort();
    for binary in runtimes.iter().filter(|binary| binary.is_file()) {
        let selected = current.is_some() && fs::canonicalize(binary).ok() == current;
        root_binary(binary, selected, &mut tally);
    }
    println!("codex-runtime-gcroots: {} rooted, {} unresolved", tally.rooted, tally.unresolved);
    u8::from(tally.unresolved_current != 0)
}

pub(super) fn store_roots(interpreter: &str, rpath: &str) -> Vec<String> {
    let roots: BTreeSet<String> = std::iter::once(interpreter)
        .chain(rpath.split(':'))
        .filter(|line| line.starts_with("/nix/store/"))
        .map(|line| line.split('/').take(4).collect::<Vec<_>>().join("/"))
        .collect();
    roots.into_iter().collect()
}

fn print_elf(flag: &str, binary: &Path) -> String {
    Command::new("patchelf").arg(flag).arg(binary).output().map(|output| super::text(&output)).unwrap_or_default()
}

fn root_binary(binary: &Path, selected: bool, tally: &mut Tally) {
    let runtime = binary.parent().and_then(Path::parent).unwrap_or(binary);
    let roots = runtime.join(".nix-gcroots");
    let interpreter = print_elf("--print-interpreter", binary);
    let rpath = print_elf("--print-rpath", binary);
    let interpreter_lines = interpreter.lines().next().unwrap_or_default();
    let rpath_entries = rpath.replace('\n', ":");
    for dep in store_roots(interpreter_lines, &rpath_entries) {
        let rooted = Path::new(&dep).exists() && {
            let name = dep.rsplit('/').next().unwrap_or_default();
            let _ = fs::create_dir_all(&roots);
            Command::new("nix-store")
                .arg("--add-root")
                .arg(roots.join(name))
                .args(["--indirect", "--realise", &dep])
                .output()
                .is_ok_and(|output| output.status.success())
        };
        if rooted {
            tally.rooted += 1;
            continue;
        }
        if Path::new(&dep).exists() {
            eprintln!("codex-runtime-gcroots: could not root {dep}");
        } else {
            eprintln!("codex-runtime-gcroots: missing {dep} (needed by {})", binary.display());
        }
        tally.unresolved += 1;
        tally.unresolved_current += u32::from(selected);
    }
}
