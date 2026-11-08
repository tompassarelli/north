use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [land, sandbox, rest @ ..] if land == "land" && sandbox == "sandbox" => ExitCode::from(north_land_sandbox::run(rest)),
        _ => {
            eprintln!("usage: north-land-sandbox land sandbox exec|child ...");
            ExitCode::from(64)
        }
    }
}
