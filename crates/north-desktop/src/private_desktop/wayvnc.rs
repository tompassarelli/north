use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use super::process::checked_output;

// wayvnc opens a clipboard device per VNC connection and destroys it on disconnect, and libwayland drops an offer still in flight to it, so the next offer's new id is invalid and wayvnc exits.
const WITHOUT_CLIPBOARD: &str = include_str!("wayvnc-without-clipboard.patch");

pub(super) fn build() -> io::Result<PathBuf> {
    let output = checked_output(
        Command::new("nix")
            .args([
                "build",
                "--no-link",
                "--print-out-paths",
                "--impure",
                "--expr",
            ])
            .arg(expression())
            .stderr(Stdio::inherit()),
    )?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|store| PathBuf::from(store).join("bin/wayvnc"))
        .find(|binary| binary.is_file())
        .ok_or_else(|| io::Error::other("the wayvnc build produced no bin/wayvnc"))
}

fn expression() -> String {
    let patch = WITHOUT_CLIPBOARD
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace("${", "\\${");
    format!(
        "let pkgs = (builtins.getFlake \"nixpkgs\").legacyPackages.${{builtins.currentSystem}}; in pkgs.wayvnc.overrideAttrs (old: {{ patches = (old.patches or [ ]) ++ [ (builtins.toFile \"wayvnc-without-clipboard.patch\" \"{patch}\") ]; }})"
    )
}
