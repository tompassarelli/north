use std::path::PathBuf;
use std::process::ExitCode;

/// The fleet facts as one JSON line, the reading `north health facts` prints for the flows that need no
/// probe of the host.
pub(super) fn run() -> ExitCode {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("health: HOME missing");
        return ExitCode::from(1);
    };
    match serde_json::to_string(&super::flows::facts::gather(&home)) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("health: facts: {error}");
            ExitCode::from(1)
        }
    }
}
