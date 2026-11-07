use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::error::NorthError;

use super::task::Task;
use super::{home, refusal};
use crate::rows::Arm;

/// One arm's private world: a clone holding nothing past the base, a local bare origin, a config
/// directory and the shims that stand in for GitHub and landing.
pub struct Sandbox {
    pub root: PathBuf,
    pub code: PathBuf,
    pub tree: PathBuf,
    pub origin: PathBuf,
    pub config: PathBuf,
    pub shims: PathBuf,
}

const GH: &str = "#!/bin/sh\necho 'gh: GitHub is unreachable in this run; work from the brief and the checkout.' >&2\nexit 1\n";
const SAFE_PUSH: &str = "#!/bin/sh\nset -e\ngit push --quiet origin HEAD:refs/heads/main\necho \"safe-push: landed $(git rev-parse HEAD) on origin/main\"\n";
const LIVE_VERBS: &str = "fleet|gate|work|task|land|lane|cloud|dispatch|safe-push|capacity|nexus-run|runtime|agent-runtime-update|release|orders|laptop|private-desktop|agent-capture|convo|grounding|graph|ledger|digest|report|threads|usage|pace|agents";
const READ_ONLY: [&str; 10] = [".claude", ".agents", ".local/share/north", ".local/share/firn", ".local/share/south", ".local/lib", ".local/bin", ".local/state/north/runtimes", ".local/state/nix", ".local/state/home-manager"];
const WRITABLE: [&str; 2] = [".cargo/registry", ".cargo/git"];
const PASSTHROUGH: &str = "#!/bin/sh\nexec \"$@\"\n";
const WRAPPER_FILE: &str = "rustc-wrapper.path";

/// The harness generations the profile's skill links point into, which live under the otherwise hidden agent state.
fn skill_generations(home: &Path) -> std::collections::BTreeSet<PathBuf> {
    let Ok(entries) = fs::read_dir(home.join(".claude/skills")) else { return Default::default() };
    entries
        .flatten()
        .filter_map(|entry| fs::canonicalize(entry.path()).ok())
        .filter_map(|target| target.ancestors().find(|dir| dir.file_name().is_some_and(|name| name.to_string_lossy().starts_with("gen-"))).map(Path::to_path_buf))
        .collect()
}

/// The machine's own tool directories, without north's, firn's or south's, from which both arms and the grader enter the dev shell.
pub fn host_path() -> String {
    format!("{}/.nix-profile/bin:/run/wrappers/bin:/run/current-system/sw/bin", home().display())
}

/// One variable of the clone's dev shell, entered with no host tools on PATH.
pub fn dev_shell_value(tree: &Path, name: &str) -> String {
    let output = Command::new("nix").args(["develop", "--command", "printenv", name]).current_dir(tree).env("PATH", host_path()).output();
    output.map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).unwrap_or_default()
}

pub fn git_text(dir: &Path, arguments: &[&str]) -> Result<String, NorthError> {
    let output = Command::new("git").arg("-C").arg(dir).args(arguments).output().map_err(NorthError::Io)?;
    if !output.status.success() {
        return Err(refusal(format!("git {} in {}: {}", arguments.join(" "), dir.display(), String::from_utf8_lossy(&output.stderr).trim())));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_owned())
}

fn script(path: &Path, text: &str) {
    north_base::foundation::shim::write_executable(path, text);
}

fn real_north() -> Result<PathBuf, NorthError> {
    let found = std::env::var_os("PATH").into_iter().flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>()).map(|dir| dir.join("north")).find(|candidate| candidate.is_file());
    found.and_then(|path| fs::canonicalize(path).ok()).ok_or_else(|| refusal("no north binary on PATH for the north arm"))
}

impl Sandbox {
    pub fn at(root: &Path, repo: &str) -> Self {
        let code = root.join("code");
        Self { tree: code.join(repo).join("main"), code, origin: root.join("origin.git"), config: root.join("config"), shims: root.join("bin"), root: root.to_owned() }
    }

    pub fn build(root: &Path, task: &Task, source: &Path, harness: Arm) -> Result<Self, NorthError> {
        if root.exists() {
            return Err(refusal(format!("{} already exists; a sandbox is never reused", root.display())));
        }
        let sandbox = Self::at(root, &task.repo);
        fs::create_dir_all(&sandbox.code).map_err(NorthError::Io)?;
        git_text(root, &["init", "--quiet", "--bare", "origin.git"])?;
        git_text(source, &["push", "--quiet", &sandbox.origin.display().to_string(), &format!("{}:refs/heads/main", task.base)])?;
        git_text(&sandbox.code, &["clone", "--quiet", &sandbox.origin.display().to_string(), &sandbox.tree.display().to_string()])?;
        for key in ["user.name", "user.email"] {
            if let Ok(value) = git_text(source, &["config", key]) {
                git_text(&sandbox.tree, &["config", key, &value])?;
            }
        }
        if git_text(&sandbox.tree, &["cat-file", "-e", &format!("{}^{{commit}}", task.landed)]).is_ok() {
            return Err(refusal(format!("{}: the landed commit is reachable inside the sandbox", task.id)));
        }
        fs::create_dir_all(&sandbox.shims).map_err(NorthError::Io)?;
        script(&sandbox.shims.join("gh"), GH);
        script(&sandbox.shims.join("safe-push"), SAFE_PUSH);
        script(&sandbox.shims.join("rustc-wrapper"), PASSTHROUGH);
        fs::write(root.join(WRAPPER_FILE), dev_shell_value(&sandbox.tree, "RUSTC_WRAPPER")).map_err(NorthError::Io)?;
        fs::create_dir_all(&sandbox.config).map_err(NorthError::Io)?;
        if harness == Arm::North {
            let north = format!("#!/bin/sh\ncase \"$1\" in\n  {LIVE_VERBS})\n    echo \"north $1: live fleet state is unreachable in this run\" >&2\n    exit 75 ;;\nesac\nexec {} \"$@\"\n", real_north()?.display());
            script(&sandbox.shims.join("north"), &north);
            for name in ["CLAUDE.md", "settings.json", "skills", "agents"] {
                symlink(home().join(".claude").join(name), sandbox.config.join(name)).map_err(NorthError::Io)?;
            }
        }
        Ok(sandbox)
    }

    /// The bubblewrap prefix. `$HOME` is empty except the harness, the toolchain state, the cargo
    /// registry and this arm's own root; the clone appears as `~/code/REPO/main` and is the only
    /// checkout; secrets other than the API key are hidden; the dev shell's compiler cache wrapper,
    /// which picks its server by checkout path, runs the compiler directly instead.
    pub fn bwrap(&self, key: &Path) -> Vec<String> {
        let home = home();
        let at = |rel: &str| home.join(rel).display().to_string();
        let mut argv: Vec<String> = ["bwrap", "--dev-bind", "/", "/", "--tmpfs", "/tmp", "--unshare-pid", "--proc", "/proc", "--die-with-parent", "--tmpfs"].map(str::to_owned).to_vec();
        argv.push(home.display().to_string());
        for kept in READ_ONLY.iter().filter(|rel| home.join(rel).exists()) {
            argv.extend(["--ro-bind".into(), at(kept), at(kept)]);
        }
        for generation in skill_generations(&home) {
            argv.extend(["--ro-bind".into(), generation.display().to_string(), generation.display().to_string()]);
        }
        for kept in WRITABLE.iter().filter(|rel| home.join(rel).exists()) {
            argv.extend(["--bind".into(), at(kept), at(kept)]);
        }
        if home.join(".claude/projects").exists() {
            argv.extend(["--tmpfs".into(), at(".claude/projects")]);
        }
        for link in [".local/state/north/current", ".nix-profile"] {
            if let Ok(target) = fs::read_link(home.join(link)) {
                argv.extend(["--symlink".into(), target.display().to_string(), at(link)]);
            }
        }
        argv.extend(["--bind".into(), self.root.display().to_string(), self.root.display().to_string()]);
        argv.extend(self.view()[4..].iter().cloned());
        let real = fs::canonicalize(key).unwrap_or_else(|_| key.to_owned());
        let generation = real.parent().unwrap_or(Path::new("/run"));
        let secrets = generation.parent().filter(|dir| dir.file_name().is_some_and(|name| name == "secrets.d")).unwrap_or(generation);
        argv.extend(["--tmpfs".into(), secrets.display().to_string(), "--ro-bind".into(), real.display().to_string(), real.display().to_string()]);
        argv.extend(["--chdir".into(), self.inside().display().to_string()]);
        argv
    }
}

impl Sandbox {
    /// The clone's path as the arm sees it.
    pub fn inside(&self) -> PathBuf {
        let rest = self.tree.strip_prefix(&self.code).map_or_else(|_| PathBuf::new(), Path::to_path_buf);
        home().join("code").join(rest)
    }

    /// The arm's view of `~/code` without its other masks: git metadata in a lane the arm made names
    /// paths under `~/code`, which outside this view are the real checkouts.
    pub fn view(&self) -> Vec<String> {
        let mut argv = vec!["bwrap".into(), "--dev-bind".into(), "/".into(), "/".into(), "--bind".into(), self.code.display().to_string(), home().join("code").display().to_string()];
        if let Some(wrapper) = fs::read_to_string(self.root.join(WRAPPER_FILE)).ok().map(|text| text.trim().to_owned()).filter(|text| !text.is_empty()) {
            argv.extend(["--ro-bind".into(), self.shims.join("rustc-wrapper").display().to_string(), wrapper]);
        }
        argv
    }

    pub fn git(&self, dir: &Path, arguments: &[&str]) -> Result<String, NorthError> {
        let view = self.view();
        let output = Command::new(&view[0]).args(&view[1..]).arg("git").arg("-C").arg(dir).args(arguments).output().map_err(NorthError::Io)?;
        if !output.status.success() {
            return Err(refusal(format!("git {} in {}: {}", arguments.join(" "), dir.display(), String::from_utf8_lossy(&output.stderr).trim())));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_owned())
    }

    /// Every checkout of the clone, as the arm sees it: the main clone first, then lanes it added.
    pub fn worktrees(&self) -> Vec<PathBuf> {
        let listing = self.git(&self.inside(), &["worktree", "list", "--porcelain"]).unwrap_or_default();
        listing.lines().filter_map(|line| line.strip_prefix("worktree ")).map(PathBuf::from).collect()
    }

    pub fn touched(&self, tree: &Path, base: &str) -> bool {
        self.git(tree, &["rev-parse", "HEAD"]).is_ok_and(|head| head != base) || self.git(tree, &["status", "--porcelain"]).is_ok_and(|status| !status.is_empty())
    }

    pub fn pushed(&self, base: &str) -> Option<String> {
        git_text(&self.origin, &["rev-parse", "refs/heads/main"]).ok().filter(|tip| tip != base)
    }
}
