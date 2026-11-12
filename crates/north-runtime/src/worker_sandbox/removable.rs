use std::path::{Path, PathBuf};

use super::except::{Entries, beneath_except};

/// Where a worker command runs, read from its environment.
pub struct Context {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub uid: u32,
    pub tmpdir: Option<PathBuf>,
    pub cache: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub runtime: Option<PathBuf>,
    pub cargo_home: Option<PathBuf>,
    pub cargo_target: Option<PathBuf>,
    pub named_lanes: Vec<PathBuf>,
    pub named_projects: Vec<PathBuf>,
    pub runtimes: Option<PathBuf>,
    pub session: Option<String>,
    pub agent_socket: Option<PathBuf>,
}

impl Context {
    /// The context of `command`, run from this process's working directory.
    pub fn from_env(command: &str) -> Self {
        let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        Self::with(command, std::env::current_dir().unwrap_or_default(), set)
    }

    /// The context of `command` when another process runs it in `cwd` with `environment`: the broker reads the handoff it starts from the request the worker command sent, not from its own directory and environment.
    pub fn from_request(command: &str, cwd: &Path, environment: &[(String, String)]) -> Self {
        let set = |name: &str| environment.iter().find(|(own, value)| own == name && !value.is_empty()).map(|(_, value)| PathBuf::from(value));
        Self::with(command, cwd.to_path_buf(), set)
    }

    fn with(command: &str, cwd: PathBuf, set: impl Fn(&str) -> Option<PathBuf>) -> Self {
        let home = set("HOME").unwrap_or_default();
        let named = named(&home, command);
        Self {
            home,
            cwd,
            uid: rustix::process::getuid().as_raw(),
            tmpdir: set("TMPDIR"),
            cache: set("XDG_CACHE_HOME"),
            config: set("XDG_CONFIG_HOME"),
            runtime: set("XDG_RUNTIME_DIR"),
            cargo_home: set("CARGO_HOME"),
            cargo_target: set("CARGO_TARGET_DIR"),
            named_lanes: named.lanes,
            named_projects: named.projects,
            runtimes: runtimes(),
            session: set("NORTH_FLEET_SESSION").and_then(|session| session.into_os_string().into_string().ok()),
            agent_socket: set(super::sockets::AGENT_ENV),
        }
    }
}

/// The trees a worker command may remove or rename files out of: its lane, the git common directory of its project and of every project it names apart from its top level and hooks (git_removable), scratch and caches, and the agent state it writes apart from the stores that hold the fleet's history (state.rs), and the Buck2 daemon directory of each of its lanes (buck.rs).
pub fn removable(context: &Context) -> Vec<PathBuf> {
    let home = &context.home;
    let mut roots: Vec<PathBuf> = ["/tmp", "/var/tmp", "/dev/shm"].map(PathBuf::from).into();
    roots.push(PathBuf::from(format!("/run/user/{}", context.uid)));
    roots.extend(context.tmpdir.clone());
    roots.push(context.cache.clone().unwrap_or_else(|| home.join(".cache")));
    roots.push(context.cargo_home.clone().unwrap_or_else(|| home.join(".cargo")));
    roots.extend(context.cargo_target.clone().map(|target| if target.is_absolute() { target } else { context.cwd.join(target) }));
    roots.extend([".rustup", ".bun", ".npm"].map(|name| home.join(name)));
    roots.extend(super::state::removable(&home.join(".local/state")));
    for common in git_common_dirs(context) {
        roots.extend(git_removable(&common));
    }
    roots.extend(projects(context).iter().filter_map(|project| check_lane(project)));
    roots.extend(lanes(context));
    roots.extend(super::buck::daemon_dirs(context));
    roots
}

/// The lanes a command run from `context` works in: the one it runs in and the ones it names.
pub(super) fn lanes(context: &Context) -> Vec<PathBuf> {
    lane(&context.home, &context.cwd).into_iter().chain(context.named_lanes.iter().cloned()).collect()
}

/// The projects a command run from `context` works in: the one it runs in, the ones it names, and those of its lanes.
pub(super) fn projects(context: &Context) -> Vec<PathBuf> {
    let parents = lanes(context).into_iter().filter_map(|lane| lane.parent().and_then(Path::parent).map(Path::to_path_buf));
    project(&context.home, &context.cwd).into_iter().chain(context.named_projects.iter().cloned()).chain(parents).collect()
}

/// The git common directories of the projects and lanes a command run from `context` works in, each once.
fn git_common_dirs(context: &Context) -> Vec<PathBuf> {
    let checkouts = projects(context).into_iter().map(|project| project.join("main")).chain(lanes(context));
    let mut commons: Vec<PathBuf> = checkouts.filter_map(|checkout| common_git_dir(&checkout)).collect();
    commons.sort();
    commons.dedup();
    commons
}

/// The git common directories of the projects a command run from `context` works in, each once, read only from their main checkouts, whose `.git` a worker may not write: a lane's gitfile and a worktree's `commondir` lie where a worker writes, so a common directory read through them is whichever one the worker names.
pub(super) fn project_git_common_dirs(context: &Context) -> Vec<PathBuf> {
    let mut commons: Vec<PathBuf> = projects(context).iter().filter_map(|project| main_common_dir(&project.join("main"))).collect();
    commons.sort();
    commons.dedup();
    commons
}

/// The common directory of the main checkout `main`: its `.git` directory, or for a `.git` gitfile the directory holding the `worktrees` directory its git directory lies in, never one a writable `commondir` names elsewhere.
fn main_common_dir(main: &Path) -> Option<PathBuf> {
    let (git_dir, common) = git_dirs(main)?;
    let own = main.join(".git").is_dir() && git_dir == common;
    let worktree = git_dir.parent().is_some_and(|worktrees| worktrees.file_name().is_some_and(|name| name == "worktrees") && worktrees.parent() == Some(common.as_path()));
    (own || worktree).then_some(common)
}

/// The directory of a git common directory that `packed-refs` links into, so a worker's git takes its lock there (packed_refs.rs).
pub(super) const PACKED: &str = "packed";

/// The directories of the git common directory `common` a worker's git writes create, rename and remove files in: objects, refs, logs, every checkout's own git directory under `worktrees` and north's records, and not `hooks`, whose files git runs for every checkout and every process, nor the top level, where `config` sits and is replaced by renaming `config.lock` over it, so a right there would reach both, nor `packed`, whose narrower rule packed_refs.rs gives. `worktrees`, `logs` and `north` are made first where missing, since a Landlock rule holds an inode.
fn git_removable(common: &Path) -> Vec<PathBuf> {
    for directory in ["worktrees", "logs", "north"] {
        let _ = std::fs::create_dir_all(common.join(directory));
    }
    beneath_except(common, &[common.join("hooks"), common.join(PACKED)], Entries::Directories)
}

/// The files at the top of each git common directory a command run from `context` works in that it writes in place: `FETCH_HEAD`, which `git fetch` in a main checkout truncates and rewrites and which is made empty where missing.
pub(super) fn git_writable(context: &Context) -> Vec<PathBuf> {
    let fetched: Vec<PathBuf> = git_common_dirs(context).into_iter().map(|common| common.join("FETCH_HEAD")).collect();
    for fetch_head in &fetched {
        let _ = std::fs::OpenOptions::new().write(true).create_new(true).open(fetch_head);
    }
    fetched
}

/// A Landlock rule holds an inode, so the done-when lane must exist before the domain is built.
fn check_lane(project: &Path) -> Option<PathBuf> {
    let lane = north_base::foundation::check_lane::of(project);
    (project.join("worktrees").is_dir() && std::fs::create_dir_all(&lane).is_ok()).then_some(lane)
}

/// The project container `cwd` lies in: `~/code/PROJECT`.
pub(super) fn project(home: &Path, cwd: &Path) -> Option<PathBuf> {
    let project = cwd.strip_prefix(home.join("code")).ok()?.components().next()?;
    Some(home.join("code").join(project))
}

/// The lane `cwd` lies in: `~/code/PROJECT/worktrees/SLUG`.
pub(super) fn lane(home: &Path, cwd: &Path) -> Option<PathBuf> {
    let project = project(home, cwd)?;
    let slug = cwd.strip_prefix(project.join("worktrees")).ok()?.components().next()?;
    Some(project.join("worktrees").join(slug))
}

/// What `command` names by absolute path (`~/code/PROJECT` and `~/code/PROJECT/worktrees/SLUG`, also written with `~` or `$HOME`).
struct Named {
    projects: Vec<PathBuf>,
    lanes: Vec<PathBuf>,
}

/// The projects and lanes `command` names. Claude Code starts every command in the session's first directory, so a worker reaches its lane with `cd LANE && ...` and the lane appears in the text, as does the project it runs git in with `git -C PROJECT/main fetch`. A computed target never does; a literal delete of another session's lane is tripwire's to refuse.
fn named(home: &Path, command: &str) -> Named {
    let code = home.join("code");
    let prefixes = [code.to_string_lossy().into_owned(), "~/code".to_owned(), "$HOME/code".to_owned(), "${HOME}/code".to_owned()];
    let word = |text: &str| -> String { text.chars().take_while(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect() };
    let mut named = Named { projects: Vec::new(), lanes: Vec::new() };
    for prefix in &prefixes {
        for (start, _) in command.match_indices(prefix.as_str()) {
            let rest = &command[start + prefix.len()..];
            let mut parts = rest.strip_prefix('/').unwrap_or("").split('/');
            let project = word(parts.next().unwrap_or(""));
            if project.is_empty() || project.starts_with('.') {
                continue;
            }
            let project = code.join(project);
            if !named.projects.contains(&project) {
                named.projects.push(project.clone());
            }
            let slug = word(parts.next().filter(|part| *part == "worktrees").and(parts.next()).unwrap_or(""));
            if slug.is_empty() || slug.starts_with('.') {
                continue;
            }
            let lane = project.join("worktrees").join(slug);
            if !named.lanes.contains(&lane) {
                named.lanes.push(lane);
            }
        }
    }
    named
}

/// The git common directory of the checkout at `checkout`, the one `git rev-parse --path-format=absolute --git-common-dir` prints there, read from the files git keeps: a `.git` directory is its own common directory, and a `.git` gitfile points at a worktree's git directory, whose `commondir` file names the common one. The grant must be this directory, since git creates its lock files there and renames them away: in a project whose `main/.git` is a gitfile, `main/.git` is a file and grants nothing.
pub(super) fn common_git_dir(checkout: &Path) -> Option<PathBuf> {
    git_dirs(checkout).map(|(_, common)| common)
}

/// Whether the checkout at `top`, a lane or the main checkout of `project`, reads a repository other than the project's own: its `.git` leads to a common directory that is not the one `project`'s main checkout keeps, as a lane's gitfile and a worktree's `commondir`, both where a worker writes, can be made to. A `top` git reads no repository from, and a project with no main checkout to compare with, are not.
pub(super) fn foreign(project: &Path, top: &Path) -> bool {
    main_common_dir(&project.join("main")).zip(common_git_dir(top)).is_some_and(|(own, read)| own != read)
}

/// The git directory of the checkout at `checkout` and its common directory, both canonical.
fn git_dirs(checkout: &Path) -> Option<(PathBuf, PathBuf)> {
    let dot = checkout.join(".git");
    let git_dir = if dot.is_dir() {
        dot
    } else {
        let pointer = std::fs::read_to_string(&dot).ok()?;
        checkout.join(pointer.trim().strip_prefix("gitdir:")?.trim())
    };
    let common = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(relative) => git_dir.join(relative.trim()),
        Err(_) => git_dir.clone(),
    };
    Some((git_dir.canonicalize().ok()?, common.canonicalize().ok()?))
}

/// Whether a command in `context` may remove the lock files a git write in `checkout` creates: those in its git directory, where the index and `HEAD` locks live, and in its common directory's `refs`, where the ref locks live (north#1172).
pub fn holds_git_locks(context: &Context, checkout: &Path) -> bool {
    let roots: Vec<PathBuf> = removable(context).into_iter().map(|root| root.canonicalize().unwrap_or(root)).collect();
    git_dirs(checkout).is_some_and(|(git_dir, common)| [git_dir, common.join("refs")].iter().all(|directory| roots.iter().any(|root| directory.starts_with(root))))
}

/// The `runtimes` directory that holds the north this process runs, wherever its host keeps north's state.
fn runtimes() -> Option<PathBuf> {
    let north = std::env::current_exe().ok()?.canonicalize().ok()?;
    north.ancestors().find(|directory| directory.file_name().is_some_and(|name| name == "runtimes")).map(Path::to_path_buf)
}
