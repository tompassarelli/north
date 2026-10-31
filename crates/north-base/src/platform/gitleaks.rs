use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::Path;

use super::process;

#[derive(Debug, Eq, PartialEq)]
pub struct Finding {
    pub rule: String,
    pub file: String,
    pub line: usize,
}

pub struct Scanner<'a> {
    pub config: &'a str,
    pub ignore: &'a Path,
}

fn read_report(path: &Path) -> Result<Vec<Finding>, String> {
    let report = std::fs::read_to_string(path).map_err(|e| format!("cannot read the gitleaks report: {e}"))?;
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&report).map_err(|e| format!("cannot parse the gitleaks report: {e}"))?;
    entries
        .iter()
        .map(|entry| {
            let rule = entry["RuleID"].as_str().ok_or("a gitleaks finding has no RuleID")?;
            Ok(Finding {
                rule: rule.to_owned(),
                file: entry["File"].as_str().unwrap_or_default().to_owned(),
                line: entry["StartLine"].as_u64().unwrap_or_default() as usize,
            })
        })
        .collect()
}

impl Scanner<'_> {
    fn args<'a>(&'a self, mode: &'a str, report: &'a str) -> Vec<&'a OsStr> {
        let mut args: Vec<&OsStr> = [
            mode, "--config", self.config, "--redact", "--no-banner", "--exit-code", "0", "--ignore-gitleaks-allow",
            "--report-format", "json", "--report-path", report, "--log-level", "error", "--max-archive-depth", "8",
        ]
        .into_iter()
        .map(OsStr::new)
        .collect();
        args.extend([OsStr::new("--gitleaks-ignore-path"), self.ignore.as_os_str()]);
        args
    }

    fn checked(output: Option<std::process::Output>, strict: bool) -> Result<(), String> {
        let output = output.ok_or("cannot run gitleaks dir")?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let skipped = strict && stderr.lines().any(|line| line.contains("ERR"));
        if output.status.success() && !skipped {
            return Ok(());
        }
        Err(format!("gitleaks dir failed or skipped a file (exit {}): {}", output.status.code().unwrap_or(-1), stderr.trim()))
    }

    pub fn dir(&self, tree: &Path, strict: bool) -> Result<Vec<Finding>, String> {
        std::fs::create_dir_all(tree).map_err(|e| format!("cannot create a scan directory: {e}"))?;
        let report = tree.with_extension("json");
        let report_arg = report.to_string_lossy();
        let mut args = self.args("dir", &report_arg);
        args.push(OsStr::new("."));
        Self::checked(process::run("gitleaks", Some(tree), &args).ok(), strict)?;
        read_report(&report)
    }
}

pub fn scan_text(config: &Path, text: &str) -> Result<BTreeSet<String>, String> {
    let scratch = tempfile::tempdir().map_err(|e| format!("cannot create a scratch directory: {e}"))?;
    let tree = scratch.path().join("text");
    let ignore = scratch.path().join("empty-ignore");
    std::fs::create_dir_all(&tree)
        .and_then(|()| std::fs::write(&ignore, ""))
        .and_then(|()| std::fs::write(tree.join("text.txt"), format!("\n{text}\n")))
        .map_err(|e| format!("cannot stage the text for the scan: {e}"))?;
    let config = config.to_string_lossy();
    let scanner = Scanner { config: &config, ignore: &ignore };
    Ok(scanner.dir(&tree, true)?.into_iter().map(|finding| finding.rule).collect())
}
