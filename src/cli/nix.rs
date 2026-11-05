use north_core::doctor_nix;
use north_core::nix_gc::{self, Env, System};
use std::process::ExitCode;

pub fn gc(arguments: &[String]) -> ExitCode {
    ExitCode::from(nix_gc::run(arguments, &Env::from_env(), &System))
}

pub fn doctor(arguments: &[String]) -> ExitCode {
    match arguments {
        [target, rest @ ..] if target == "nix" => ExitCode::from(doctor_nix::run(rest, &Env::from_env(), &System)),
        _ => {
            eprintln!("usage: north doctor nix [--lock FILE [--allow FILE]]");
            ExitCode::from(2)
        }
    }
}
