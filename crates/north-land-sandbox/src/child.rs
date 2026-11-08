use std::os::unix::process::CommandExt;
use std::process::Command;

use nix::sys::{ptrace, signal};

use crate::SANDBOX_FAILED;
use crate::options::Options;
use crate::{fence, recorder};

pub fn run(arguments: &[String]) -> u8 {
    match prepare(arguments) {
        Ok(mut command) => {
            let error = command.exec();
            eprintln!("north land sandbox child: cannot run {:?}: {error}", command.get_program());
            if error.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
        }
        Err(reason) => {
            eprintln!("north land sandbox child: {reason}");
            SANDBOX_FAILED
        }
    }
}

fn prepare(arguments: &[String]) -> Result<Command, String> {
    let options = Options::parse(arguments)?;
    let (program, rest) = options.command.split_first().ok_or("no command after --")?;
    if let Some(path_set) = &options.enforce {
        let tree = options.tree.as_ref().ok_or("--enforce needs --tree")?;
        let scratch = options.scratch.as_ref().ok_or("--enforce needs --scratch")?;
        fence::restrict(path_set, tree, scratch, std::path::Path::new(program))?;
    }
    if options.trace {
        ptrace::traceme().map_err(|error| format!("ptrace: {error}"))?;
        signal::raise(signal::Signal::SIGSTOP).map_err(|error| format!("stop for the tracer: {error}"))?;
        recorder::install()?;
    }
    let mut command = Command::new(program);
    command.args(rest);
    Ok(command)
}
