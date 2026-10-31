mod archive;
mod args;
mod blake;
mod commands;
mod compress;
mod db;
mod discover;
mod env;
mod failure;
mod hit;
mod index;
mod json;
mod lines;
mod query;
mod rank;
mod records;
mod render;
mod sessions;
mod text;
#[cfg(test)]
mod core_tests;
#[cfg(test)]
mod tests;

use std::io::{BufWriter, IsTerminal, Write};
use std::time::Instant;

use args::Args;
use env::Env;
use failure::{Failure, Result};
use render::Palette;

pub struct Console<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
}

pub fn run(arguments: &[String]) -> u8 {
    let env = Env::from_env();
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut err = std::io::stderr();
    let tty = std::io::stdout().is_terminal();
    let code = execute(&env, arguments, &mut Console { out: &mut out, err: &mut err }, tty);
    let _ = out.flush();
    code
}

fn execute(env: &Env, arguments: &[String], io: &mut Console, tty: bool) -> u8 {
    let args = match args::parse(arguments) {
        Ok(args) => args,
        Err(message) => return commands::usage_failure(io, &format!("error: {message}")),
    };
    let pal = Palette::new(args.color == "always" || (args.color == "auto" && tty));
    match dispatch(env, io, &args, &pal) {
        Ok(code) => code,
        Err(Failure::Pipe) => 0,
        Err(failure) => {
            let _ = writeln!(io.err, "convo: {failure}");
            failure.exit_code()
        }
    }
}

fn dispatch(env: &Env, io: &mut Console, args: &Args, pal: &Palette) -> Result<u8> {
    if args.help {
        write!(io.out, "{}", args::HELP)?;
        return Ok(0);
    }
    match args.command.as_str() {
        "restore" => compress::restore(env, io, args),
        "compress" => {
            if !(args.dry_run || args.no_update) && env.db().exists() {
                index::refresh(env, args.quiet, false, env.lock_wait, false)?;
            }
            compress::compress(env, io, args, pal)
        }
        "index" => rebuild(env, io, args),
        _ => query(env, io, args, pal),
    }
}

fn rebuild(env: &Env, io: &mut Console, args: &Args) -> Result<u8> {
    let started = Instant::now();
    let Some(done) = index::refresh(env, args.quiet, args.verbose, env.lock_wait, args.full)? else {
        writeln!(io.err, "convo: another index run holds the lock")?;
        return Ok(2);
    };
    writeln!(
        io.out,
        "indexed {} messages from {} changed files ({:.2} GiB read) in {:.1}s",
        done.messages, done.files, done.bytes as f64 / (1u64 << 30) as f64, started.elapsed().as_secs_f64()
    )?;
    Ok(0)
}

fn query(env: &Env, io: &mut Console, args: &Args, pal: &Palette) -> Result<u8> {
    if args.command == "search" && args.query.is_empty() && !args.has_filter() {
        write!(io.out, "{}", args::HELP)?;
        return Ok(0);
    }
    if !env.db().exists() {
        writeln!(io.err, "convo: no index yet; build it once with `north convo index` (reads every transcript; run it under a machine-capacity lease)")?;
        return Ok(2);
    }
    let mut issue = args.no_update.then_some("update disabled");
    if !args.no_update && index::refresh(env, true, false, env.lock_wait, false)?.is_none() {
        issue = Some("index lock busy");
    }
    let con = db::connect(&env.db(), &env.state, true)?;
    let code = match args.command.as_str() {
        "status" => return sessions::status(&con, env, io),
        "show" => return commands::show(&con, io, args, pal),
        "session" => sessions::session(&con, io, args, pal)?,
        _ => commands::search(&con, env, io, args, pal)?,
    };
    if let Some(issue) = issue {
        if code == 1 {
            writeln!(io.err, "convo: refresh inconclusive ({issue}); a miss proves nothing")?;
            return Ok(2);
        }
        if code == 0 && !args.no_update {
            writeln!(io.err, "convo: refresh inconclusive ({issue}); results may be stale")?;
        }
    }
    Ok(code)
}
