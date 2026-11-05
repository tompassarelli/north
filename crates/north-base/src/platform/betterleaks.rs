use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

use super::process;

pub const SECRET_CONFIG: &str = include_str!("betterleaks-secrets.toml");
pub const CONFIG_ENV: &str = "BETTERLEAKS_CONFIG_TOML";
const QUICK: [&str; 7] = ["--no-banner", "--redact", "--regex-engine", "stdlib", "--log-level", "error", "--no-color"];

pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().to_ascii_uppercase().contains("LEAKS_CONFIG") {
            command.env_remove(name);
        }
    }
    command.env(CONFIG_ENV, SECRET_CONFIG);
    command
}

fn arguments(mode: &str, extra: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    [mode].into_iter().chain(QUICK).map(OsString::from).chain(extra).collect()
}

pub fn history_args(range: &str) -> Vec<OsString> {
    arguments("git", ["--trust-local-git-config".into(), format!("--log-opts={range}").into()])
}

pub fn directory_args(source: &Path) -> Vec<OsString> {
    arguments("dir", [source.as_os_str().to_owned()])
}

pub fn stdin_args() -> Vec<OsString> {
    arguments("stdin", [])
}

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
    let report = std::fs::read_to_string(path).map_err(|e| format!("cannot read the betterleaks report: {e}"))?;
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&report).map_err(|e| format!("cannot parse the betterleaks report: {e}"))?;
    entries
        .iter()
        .map(|entry| {
            let rule = entry["RuleID"].as_str().ok_or("a betterleaks finding has no RuleID")?;
            Ok(Finding {
                rule: rule.to_owned(),
                file: entry["File"].as_str().unwrap_or_default().to_owned(),
                line: entry["StartLine"].as_u64().unwrap_or_default() as usize,
            })
        })
        .collect()
}

// the upstream spells its flag to ignore inline allow comments after its old name
impl Scanner<'_> {
    fn args<'a>(&'a self, mode: &'a str, report: &'a str) -> Vec<&'a OsStr> {
        let mut args: Vec<&OsStr> = [
            mode, "--config", self.config, "--redact", "--no-banner", "--exit-code", "0", "--ignore-gitleaks-allow",
            "--report-format", "json", "--report-path", report, "--log-level", "error", "--max-archive-depth", "8", "--regex-engine", "stdlib",
        ]
        .into_iter()
        .map(OsStr::new)
        .collect();
        args.extend([OsStr::new("-i"), self.ignore.as_os_str()]);
        args
    }

    fn checked(output: Option<std::process::Output>, strict: bool) -> Result<(), String> {
        let output = output.ok_or("cannot run betterleaks dir")?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let skipped = strict && stderr.lines().any(|line| line.contains("ERR"));
        if output.status.success() && !skipped {
            return Ok(());
        }
        Err(format!("betterleaks dir failed or skipped a file (exit {}): {}", output.status.code().unwrap_or(-1), stderr.trim()))
    }

    pub fn dir(&self, tree: &Path, strict: bool) -> Result<Vec<Finding>, String> {
        std::fs::create_dir_all(tree).map_err(|e| format!("cannot create a scan directory: {e}"))?;
        let report = tree.with_extension("json");
        let report_arg = report.to_string_lossy();
        let mut args = self.args("dir", &report_arg);
        args.push(OsStr::new("."));
        Self::checked(process::run("betterleaks", Some(tree), &args).ok(), strict)?;
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

#[cfg(test)]
mod tests {
    use super::{command, directory_args, history_args, stdin_args};
    use std::io::Write;
    use std::path::Path;
    use std::process::Stdio;

    fn pem(kind: &str) -> String {
        format!("-----BEGIN {kind} PRIVATE KEY-----\n{}\n-----END {kind} PRIVATE KEY-----\n", "bXlmYWtla2V5Ym9keQ0".repeat(6))
    }

    fn planted() -> Vec<(&'static str, String)> {
        vec![
            ("age", format!("AGE-SECRET-KEY-1{}\n", "QPZRY9X8".repeat(7) + "QP")),
            ("ssh", pem("OPENSSH")),
            ("github app key", pem("RSA")),
            ("github token", format!("GH_TOKEN=ghp_{}\n", "aB3dE6gH".repeat(5))),
            ("github fine-grained token", format!("GH_TOKEN=github_pat_{}a\n", "aB3dE6gH_".repeat(9))),
            ("hetzner", format!("HCLOUD_TOKEN={}\n", "aB3dE6gH".repeat(8))),
            ("digitalocean", format!("DIGITALOCEAN_TOKEN=dop_v1_{}\n", "a1b2c3d4".repeat(8))),
            ("cloudflare", format!("CLOUDFLARE_API_TOKEN={}\n", "aB3-dE6_".repeat(5))),
            ("anthropic", format!("ANTHROPIC_API_KEY=sk-ant-api03-{}\n", "Ab3_".repeat(25))),
        ]
    }

    fn git(directory: &Path, arguments: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(directory)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(arguments)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn history_clean(directory: &Path, range: &str) -> bool {
        command("betterleaks").current_dir(directory).args(history_args(range)).status().unwrap().success()
    }

    #[test]
    fn every_guarded_secret_type_is_caught_in_a_range_and_clean_text_passes() {
        let scratch = tempfile::tempdir().unwrap();
        let repo = scratch.path();
        git(repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("notes.txt"), "tokens are described in docs, nothing secret here\n").unwrap();
        git(repo, &["add", "notes.txt"]);
        git(repo, &["commit", "-qm", "notes"]);
        let clean_base = git(repo, &["rev-parse", "HEAD"]);
        for (label, body) in planted() {
            let base = git(repo, &["rev-parse", "HEAD"]);
            let file = format!("planted {label}");
            std::fs::write(repo.join(&file), body).unwrap();
            git(repo, &["add", &file]);
            git(repo, &["commit", "-qm", label]);
            assert!(!history_clean(repo, &format!("{base}..HEAD")), "{label} was not caught");
        }
        git(repo, &["rm", "-qr", "--", "."]);
        git(repo, &["commit", "-qm", "remove it again"]);
        assert!(!history_clean(repo, &format!("{clean_base}..HEAD")), "a secret removed by a later commit slipped through");
        assert!(history_clean(repo, &format!("{clean_base}..{clean_base}")));
    }

    #[test]
    fn text_and_annotation_scans_catch_a_secret_and_pass_clean_text() {
        let (_, token) = planted().remove(3);
        let mut scan = command("betterleaks").args(stdin_args()).stdin(Stdio::piped()).spawn().unwrap();
        scan.stdin.take().unwrap().write_all(token.as_bytes()).unwrap();
        assert!(!scan.wait().unwrap().success(), "item text with a token passed");
        let mut clean = command("betterleaks").args(stdin_args()).stdin(Stdio::piped()).spawn().unwrap();
        clean.stdin.take().unwrap().write_all(b"nothing secret here\n").unwrap();
        assert!(clean.wait().unwrap().success());
        let scratch = tempfile::tempdir().unwrap();
        std::fs::write(scratch.path().join("tag-object.txt"), token).unwrap();
        assert!(!command("betterleaks").args(directory_args(scratch.path())).status().unwrap().success(), "a tag annotation with a token passed");
    }
}
