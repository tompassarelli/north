use std::path::PathBuf;
use std::process::ExitCode;

use north_base::lifecycle::Retire;

/// The fleet facts as one JSON line, the reading `north health facts` prints for the flows that need no probe of
/// the host, after this host retires its own dead holders' rows so the laptop's copy of them reads settled.
pub(super) fn run(retire: &[Retire]) -> ExitCode {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("health: HOME missing");
        return ExitCode::from(1);
    };
    if let Err(error) = super::flows::facts::retire_gone(&home, retire) {
        eprintln!("health: retire: {error}");
    }
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
