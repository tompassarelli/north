use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use north_agent_starts::{Spool, Starter, record};

const USAGE: &str = "Usage: north-agent-starts serve --spool DIR --agent USER --home DIR --entry PATH --machinectl PATH | take --home DIR";
const POLL: Duration = Duration::from_secs(1);
const ENTRY_WAIT: Duration = Duration::from_secs(60);

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match arguments.split_first() {
        Some((verb, rest)) if verb == "serve" => flags(rest, &["--spool", "--agent", "--home", "--entry", "--machinectl"]).and_then(|values| serve(&values)),
        Some((verb, rest)) if verb == "take" => flags(rest, &["--home"]).and_then(|values| take(Path::new(&values[0]))),
        _ => Err(USAGE.into()),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("north-agent-starts: {why}");
            ExitCode::from(2)
        }
    }
}

fn flags(rest: &[String], names: &[&str]) -> Result<Vec<String>, String> {
    let mut values = vec![None; names.len()];
    for pair in rest.chunks(2) {
        let [flag, value] = pair else { return Err(USAGE.into()) };
        let slot = names.iter().position(|name| name == flag).ok_or_else(|| USAGE.to_owned())?;
        if values[slot].replace(value.clone()).is_some() {
            return Err(USAGE.into());
        }
    }
    values.into_iter().collect::<Option<Vec<_>>>().ok_or_else(|| USAGE.to_owned())
}

fn serve(values: &[String]) -> Result<(), String> {
    rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable).map_err(|error| format!("cannot make the starter undumpable: {error}"))?;
    let spool = Spool::at(Path::new(&values[0]));
    let starter = Starter { agent: values[1].clone(), home: PathBuf::from(&values[2]), entry: PathBuf::from(&values[3]), machinectl: PathBuf::from(&values[4]), wait: ENTRY_WAIT };
    let mut log = std::io::stderr();
    loop {
        if let Err(error) = starter.serve_once(&spool, &mut log) {
            eprintln!("north-agent-starts: cannot read the spool: {error}");
        }
        std::thread::sleep(POLL);
    }
}

fn take(home: &Path) -> Result<(), String> {
    let request = record(&mut std::io::stdin(), &mut std::io::stdout(), home)?;
    println!("recorded {}", request.name);
    Ok(())
}
