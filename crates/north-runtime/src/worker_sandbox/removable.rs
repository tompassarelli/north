use std::path::{Path, PathBuf};

/// Where a worker command runs, read from its environment.
pub struct Context {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub uid: u32,
    pub tmpdir: Option<PathBuf>,
    pub cache: Option<PathBuf>,
    pub cargo_home: Option<PathBuf>,
    pub cargo_target: Option<PathBuf>,
    pub named_lanes: Vec<PathBuf>,
}

impl Context {
    /// The context of `command`, run from this process's working directory.
    pub fn from_env(command: &str) -> Self {
        let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        Self {
            home: set("HOME").unwrap_or_default(),
            cwd: std::env::current_dir().unwrap_or_default(),
            uid: rustix::process::getuid().as_raw(),
            tmpdir: set("TMPDIR"),
            cache: set("XDG_CACHE_HOME"),
            cargo_home: set("CARGO_HOME"),
            cargo_target: set("CARGO_TARGET_DIR"),
            named_lanes: named_lanes(&set("HOME").unwrap_or_default(), command),
        }
    }
}

/// The trees a worker command may remove or rename files out of: its lane, its project's git directory, whose lock files, packs and maintenance files git renames from every checkout, scratch and caches, and agent state apart from the account profiles that hold transcripts and logins.
pub fn removable(context: &Context) -> Vec<PathBuf> {
    let home = &context.home;
    let mut roots: Vec<PathBuf> = ["/tmp", "/var/tmp", "/dev/shm"].map(PathBuf::from).into();
    roots.push(PathBuf::from(format!("/run/user/{}", context.uid)));
    roots.extend(context.tmpdir.clone());
    roots.push(context.cache.clone().unwrap_or_else(|| home.join(".cache")));
    roots.push(context.cargo_home.clone().unwrap_or_else(|| home.join(".cargo")));
    roots.extend(context.cargo_target.clone().map(|target| if target.is_absolute() { target } else { context.cwd.join(target) }));
    roots.extend([".rustup", ".bun", ".npm"].map(|name| home.join(name)));
    let state = home.join(".local/state");
    roots.extend(beneath_except(&state, &[state.join("north/profiles"), state.join("north/claude-tokens")]));
    for lane in lane(home, &context.cwd).into_iter().chain(context.named_lanes.iter().cloned()) {
        roots.extend(lane.parent().and_then(Path::parent).map(|project| project.join("main/.git")));
        roots.push(lane);
    }
    roots.extend(project(home, &context.cwd).map(|project| project.join("main/.git")));
    roots
}

/// The project container `cwd` lies in: `~/code/PROJECT`.
fn project(home: &Path, cwd: &Path) -> Option<PathBuf> {
    let project = cwd.strip_prefix(home.join("code")).ok()?.components().next()?;
    Some(home.join("code").join(project))
}

/// The lane `cwd` lies in: `~/code/PROJECT/worktrees/SLUG`.
fn lane(home: &Path, cwd: &Path) -> Option<PathBuf> {
    let project = project(home, cwd)?;
    let slug = cwd.strip_prefix(project.join("worktrees")).ok()?.components().next()?;
    Some(project.join("worktrees").join(slug))
}

/// The lanes `command` names by absolute path (`~/code/PROJECT/worktrees/SLUG`, also written with `~` or `$HOME`). Claude Code starts every command in the session's first directory, so a worker reaches its lane with `cd LANE && ...` and the lane appears in the text. A computed target never does; a literal delete of another session's lane is tripwire's to refuse.
fn named_lanes(home: &Path, command: &str) -> Vec<PathBuf> {
    let code = home.join("code");
    let prefixes = [code.to_string_lossy().into_owned(), "~/code".to_owned(), "$HOME/code".to_owned(), "${HOME}/code".to_owned()];
    let mut lanes = Vec::new();
    for prefix in &prefixes {
        for (start, _) in command.match_indices(prefix.as_str()) {
            let rest = &command[start + prefix.len()..];
            let mut parts = rest.strip_prefix('/').unwrap_or("").split('/');
            let (Some(project), Some("worktrees"), Some(slug)) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            let slug: String = slug.chars().take_while(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect();
            if project.is_empty() || project.starts_with('.') || slug.is_empty() || slug.starts_with('.') || project.contains(char::is_whitespace) {
                continue;
            }
            let lane = code.join(project).join("worktrees").join(slug);
            if !lanes.contains(&lane) {
                lanes.push(lane);
            }
        }
    }
    lanes
}

/// The directories under `root` other than the `protected` paths, by real path since a Landlock rule holds an inode: a symlink is followed only toward a protected path inside it (`~/.local/state/north` links to `~/code/north-data`, which holds the profiles) and never granted itself, and a plain file is never a rule because removal is a right of its directory.
fn beneath_except(root: &Path, protected: &[PathBuf]) -> Vec<PathBuf> {
    let protected: Vec<PathBuf> = protected.iter().filter_map(|path| path.canonicalize().ok()).collect();
    root.canonicalize().map_or_else(|_| Vec::new(), |root| directories_except(&root, &protected))
}

fn directories_except(root: &Path, protected: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for path in std::fs::read_dir(root).into_iter().flatten().flatten().map(|entry| entry.path()) {
        let Ok(real) = path.canonicalize() else {
            continue;
        };
        if !real.is_dir() || protected.contains(&real) {
            continue;
        }
        if protected.iter().any(|kept| kept.starts_with(&real)) {
            roots.extend(directories_except(&real, protected));
        } else if !path.is_symlink() {
            roots.push(real);
        }
    }
    roots
}
