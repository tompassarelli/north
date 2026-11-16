use std::path::{Path, PathBuf};

use super::Context;
use super::except::{Entries, beneath_except};

const OUTPUTS: [&str; 2] = ["target", "node_modules"];

/// The build output a worker command may not run by its path, since it holds the code a lane's dependencies bring: `target/` and `node_modules/` of the lane and main checkouts the command runs in or names (a `node_modules/` one level down too), the cargo target it builds into and the north builds head-north keeps. A path not yet built is held the same way, so a binary a build writes there during the command stays refused.
pub fn build_outputs(context: &Context) -> Vec<PathBuf> {
    let mut outputs: Vec<PathBuf> = checkouts(context).iter().flat_map(|checkout| outputs_of(checkout)).collect();
    outputs.extend(context.cargo_target.clone().map(|target| if target.is_absolute() { target } else { context.cwd.join(target) }));
    outputs.push(context.cache.clone().unwrap_or_else(|| context.home.join(".cache")).join("north/head-north"));
    outputs
}

/// Every file a worker command may execute: all of them but the build output, with each Buck2 lane's `buck-out/` made first so it is one of them (buck.rs).
pub fn executable(context: &Context) -> Vec<PathBuf> {
    super::buck::make_build_output(context);
    beneath_except(Path::new("/"), &build_outputs(context), Entries::All)
}

fn checkouts(context: &Context) -> Vec<PathBuf> {
    let code = context.home.join("code");
    let mut checkouts: Vec<PathBuf> = Vec::new();
    let mut add = |checkout: PathBuf| {
        if checkout.is_dir() && !checkouts.contains(&checkout) {
            checkouts.push(checkout);
        }
    };
    if let Ok(relative) = context.cwd.strip_prefix(&code) {
        let parts: Vec<_> = relative.components().take(3).map(|part| part.as_os_str().to_owned()).collect();
        if let [project, rest @ ..] = parts.as_slice() {
            add(code.join(project).join("main"));
            if let [worktrees, slug] = rest
                && worktrees == "worktrees"
            {
                add(code.join(project).join("worktrees").join(slug));
            }
        }
    }
    for project in &context.named_projects {
        add(project.join("main"));
    }
    for lane in &context.named_lanes {
        add(lane.clone());
    }
    checkouts
}

fn outputs_of(checkout: &Path) -> Vec<PathBuf> {
    let mut outputs: Vec<PathBuf> = OUTPUTS.iter().map(|name| checkout.join(name)).collect();
    let nested = std::fs::read_dir(checkout).into_iter().flatten().flatten().map(|entry| entry.path());
    outputs.extend(nested.filter(|path| path.is_dir() && !path.is_symlink() && !OUTPUTS.iter().any(|name| path.ends_with(name))).map(|path| path.join("node_modules")).filter(|path| path.exists()));
    outputs
}
