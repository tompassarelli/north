use std::path::Path;

use super::{Arguments, Config, affected, block_on, clippy_first, run_added, say};

const NO_VERDICT: i32 = 75;

pub fn refuse_queue_checks(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let config = Config::load(root, head);
    let mut landing = config.commands(root, base, head)?;
    landing.extend(config.after_set().commands(root, base, head)?);
    let clippy = clippy_first::plan(&landing, |command| !config.is_inferred(command)).map(|plan| plan.clippy);
    let queued = config.queue_set().commands(root, base, head)?;
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
    if status == NO_VERDICT {
        say("safe-push: a check before queueing reached no verdict (exit 75: no build host answered), which is not the lane's fault; queueing the lane");
        return Ok(());
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
