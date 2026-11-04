use std::path::Path;
use std::process::Command;

use super::candidate;

pub const TOKEN_ENV: &str = "NORTH_LAND_PUSH_TOKEN";
pub const MINT: &str = "gh-app-token";
const KEY: &str = "landing.push-app";
const HELPER: &str = "!f() { test \"$1\" = get && printf 'username=x-access-token\\npassword=%s\\n' \"$NORTH_LAND_PUSH_TOKEN\"; }; f";
const SCRUBBED: &[&str] = &["GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"];

#[derive(Debug, PartialEq, Eq)]
pub enum Credential {
    Inherited,
    App(String),
}

pub fn choose(configured: &[&str]) -> Result<Credential, String> {
    match configured {
        [] => Ok(Credential::Inherited),
        [name] if !name.is_empty() && name.chars().all(|character| character.is_ascii_alphanumeric() || character == '-') => Ok(Credential::App((*name).to_owned())),
        [name] => Err(format!("{KEY} names {name:?}, which is not an App name")),
        _ => Err(format!("{KEY} is set more than once")),
    }
}

fn configured(worktree: &Path, base: &str) -> Result<Credential, String> {
    let blob = format!("{base}:.safe-push");
    if !candidate::output(worktree, &["cat-file", "-e", &blob])?.status.success() {
        return Ok(Credential::Inherited);
    }
    let read = candidate::output(worktree, &["config", "--blob", &blob, "--get-all", KEY])?;
    match read.status.code() {
        Some(0) => choose(&String::from_utf8_lossy(&read.stdout).lines().map(str::trim).collect::<Vec<_>>()),
        Some(1) => Ok(Credential::Inherited),
        _ => Err(format!("cannot read {KEY} from {blob}: {}", String::from_utf8_lossy(&read.stderr).trim())),
    }
}

pub fn mint(program: &[String], app: &str) -> Result<String, String> {
    let (command, leading) = program.split_first().ok_or("no token program is configured")?;
    let output = Command::new(command).args(leading).args(["--app", app]).output().map_err(|error| format!("{command}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{command} --app {app} failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if token.is_empty() || token.contains(char::is_whitespace) {
        return Err(format!("{command} --app {app} printed no token"));
    }
    Ok(token)
}

pub fn for_push(worktree: &Path, base: &str, program: &[String]) -> Result<Option<String>, String> {
    match configured(worktree, base)? {
        Credential::Inherited => Ok(None),
        Credential::App(app) => mint(program, &app).map(Some),
    }
}

pub fn apply(push: &mut Command, token: Option<&str>) {
    let Some(token) = token else { return };
    push.args(["-c", "credential.helper=", "-c", "credential.https://github.com.helper=", "-c", &format!("credential.https://github.com.helper={HELPER}")]);
    for name in SCRUBBED {
        push.env_remove(name);
    }
    push.env(TOKEN_ENV, token).env("GIT_TERMINAL_PROMPT", "0");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helper_overrides_every_inherited_helper() {
        let scratch = std::env::temp_dir().join(format!("north-credential-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).unwrap();
        let global = scratch.join("gitconfig");
        std::fs::write(&global, "[credential \"https://github.com\"]\n\thelper = !f() { printf 'username=decoy\\npassword=decoy\\n'; }; f\n").unwrap();
        let mut fill = Command::new("git");
        apply(&mut fill, Some("app-secret"));
        let mut child = fill.args(["credential", "fill"]).env("GIT_CONFIG_GLOBAL", &global).env("GIT_CONFIG_NOSYSTEM", "1").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
        std::io::Write::write_all(child.stdin.as_mut().unwrap(), b"protocol=https\nhost=github.com\npath=tompassarelli/north\n\n").unwrap();
        let output = child.wait_with_output().unwrap();
        std::fs::remove_dir_all(&scratch).unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout), "protocol=https\nhost=github.com\nusername=x-access-token\npassword=app-secret\n");
    }

    #[test]
    fn no_setting_keeps_the_inherited_credential() {
        assert_eq!(choose(&[]), Ok(Credential::Inherited));
    }

    #[test]
    fn one_app_name_selects_the_app() {
        assert_eq!(choose(&["lander"]), Ok(Credential::App("lander".to_owned())));
    }

    #[test]
    fn a_malformed_or_repeated_setting_is_an_error_not_a_fallback() {
        for bad in [vec![""], vec!["a b"], vec!["a;b"], vec!["lander", "socrates"]] {
            assert!(choose(&bad).is_err(), "{bad:?}");
        }
    }
}
