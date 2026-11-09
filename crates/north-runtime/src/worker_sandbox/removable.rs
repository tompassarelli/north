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
    pub cargo_home: Option<PathBuf>,
    pub cargo_target: Option<PathBuf>,
    pub named_lanes: Vec<PathBuf>,
    pub named_projects: Vec<PathBuf>,
}

impl Context {
    /// The context of `command`, run from this process's working directory.
    pub fn from_env(command: &str) -> Self {
        let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        let named = named(&set("HOME").unwrap_or_default(), command);
        Self {
            home: set("HOME").unwrap_or_default(),
            cwd: std::env::current_dir().unwrap_or_default(),
            uid: rustix::process::getuid().as_raw(),
            tmpdir: set("TMPDIR"),
            cache: set("XDG_CACHE_HOME"),
            config: set("XDG_CONFIG_HOME"),
            cargo_home: set("CARGO_HOME"),
            cargo_target: set("CARGO_TARGET_DIR"),
            named_lanes: named.lanes,
            named_projects: named.projects,
        }
    }
}

/// The trees a worker command may remove or rename files out of: its lane, the git common directory of its project and of every project it names, whose lock files, packs and maintenance files git renames from every checkout, scratch and caches, and agent state apart from the account profiles that hold transcripts and logins.
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
    roots.extend(beneath_except(&state, &[state.join("north/profiles"), state.join("north/claude-tokens")], Entries::Directories));
    let lanes: Vec<PathBuf> = lane(home, &context.cwd).into_iter().chain(context.named_lanes.iter().cloned()).collect();
    let projects = project(home, &context.cwd).into_iter().chain(context.named_projects.iter().cloned()).chain(lanes.iter().filter_map(|lane| lane.parent().and_then(Path::parent)).map(Path::to_path_buf));
    for project in projects {
        roots.extend(common_git_dir(&project.join("main")));
        roots.push(project.join("main/.git"));
        roots.extend(check_lane(&project));
    }
    for lane in lanes {
        roots.extend(common_git_dir(&lane));
        roots.push(lane);
    }
    roots
}

/// A Landlock rule holds an inode, so the done-when lane must exist before the domain is built.
fn check_lane(project: &Path) -> Option<PathBuf> {
    let lane = north_base::foundation::check_lane::of(project);
    (project.join("worktrees").is_dir() && std::fs::create_dir_all(&lane).is_ok()).then_some(lane)
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
fn common_git_dir(checkout: &Path) -> Option<PathBuf> {
    let dot = checkout.join(".git");
    let git_dir = if dot.is_dir() {
        dot
    } else {
        let pointer = std::fs::read_to_string(&dot).ok()?;
        checkout.join(pointer.trim().strip_prefix("gitdir:")?.trim())
    };
    let common = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(relative) => git_dir.join(relative.trim()),
        Err(_) => git_dir,
    };
    common.canonicalize().ok()
}
