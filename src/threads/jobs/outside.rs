//! The ledger's outside effects: gitleaks before text is stored or posted, gh for issue comments.
use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::threads::catalog::CATALOG;

pub(crate) struct Live;

impl super::Outside for Live {
    fn scan(&self, text: &str) -> Result<(), String> {
        gitleaks_clean(text)
    }

    fn comment(&self, repo: &str, number: u64, body: &str) -> Result<(), String> {
        let args = ["issue", "comment", &number.to_string(), "-R", &format!("{}/{repo}", CATALOG.owner), "--body", body].map(str::to_owned);
        crate::platform::github::run_patiently(&args, "NorthThreads").map(drop)
    }
}

pub(crate) fn gitleaks_clean(text: &str) -> Result<(), String> {
    let mut command = Command::new("gitleaks");
    command.args(["stdin", "--no-banner", "--redact", "-l", "error"]).current_dir("/").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err("gitleaks is not on PATH; items are scanned for secrets before they are stored".into());
        }
        Err(error) => return Err(error.to_string()),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let bytes = text.as_bytes().to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }
    let output = child.wait_with_output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("gitleaks found a secret in the item text; credentials stay in Bitwarden, so nothing was stored".into());
    }
    Ok(())
}
