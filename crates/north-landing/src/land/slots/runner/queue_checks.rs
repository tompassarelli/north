use std::path::Path;

use super::{Arguments, Config, affected, block_on, clippy_first, run_added, say, shared_build};
use north_base::foundation::exit::NO_VERDICT_STATUS;


pub fn refuse_queue_checks(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let config = Config::load(root, head);
    let mut landing = config.commands(root, base, head)?;
    landing.extend(config.after_set().commands(root, base, head)?);
    let clippy = clippy_first::plan(&landing, |command| !config.is_inferred(command)).map(|plan| plan.clippy);
    let after_lines = if crate::landing_refusals::enforcing() { config.queue_set().commands(root, base, head)? } else { Vec::new() };
    let queued: Vec<String> = after_lines.iter().map(|command| shared_build::built(command)).collect();
    let commands: Vec<String> = clippy.iter().cloned().chain(queued.into_iter().filter(|command| clippy.as_ref() != Some(command))).collect();
    if commands.is_empty() {
        return Ok(());
    }
    say(&format!("safe-push: running {} check(s) before queueing the lane: {}", commands.len(), commands.join("; ")));
    let arguments = Arguments { base: base.to_owned(), head: head.to_owned(), ..Arguments::default() };
    let status = block_on(run_added(root.to_owned(), arguments, config, commands)).map_err(|error| error.to_string())?;
    if status == 0 {
        return Ok(());
    }
    if status == NO_VERDICT_STATUS {
        return Err("the checks before queueing reached no verdict (exit 75: no capacity on a build host or here); the lane is not queued uncompiled, so run safe-push again".to_owned());
    }
    let rerun = match clippy {
        Some(_) => {
            let crates = affected::scope(root, base, head).map_or_else(|_| "--workspace".to_owned(), |scope| scope.changed_arguments());
            format!("; to rerun clippy alone: cargo clippy --all-targets {crates} -- -D warnings")
        }
        None => String::new(),
    };
    Err(format!("a check before queueing exited {status}; fix the failure above{rerun}"))
}
