use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Another machine a session's provider CLI runs on, reached through `ssh`; the session host and
/// transcript stay on this one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub name: String,
    pub ssh: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_ssh: Option<String>,
}

impl Machine {
    pub fn session_login(&self) -> &str {
        self.agent_ssh.as_deref().unwrap_or(&self.ssh)
    }

    pub fn session_home(&self, operator_home: &Path) -> Option<PathBuf> {
        self.agent_ssh.as_deref().and_then(|login| Self::login_home(login, operator_home))
    }

    pub fn session_path(&self, path: &Path, operator_home: &Path) -> PathBuf {
        match (self.session_home(operator_home), path.strip_prefix(operator_home)) {
            (Some(home), Ok(rest)) => home.join(rest),
            _ => path.to_path_buf(),
        }
    }

    /// The home of the Unix user a session login names, None when it names none, names one that is no plain user name, or names the operator, and None too when the operator's home is unknown, so a host never counts the operator's login as an agent's.
    pub fn login_home(login: &str, operator_home: &Path) -> Option<PathBuf> {
        let user = login.split_once('@').map(|(user, _)| user).filter(|user| unix_user(user))?;
        let home = PathBuf::from(format!("/home/{user}"));
        (operator_home.is_absolute() && home != operator_home).then_some(home)
    }
}

fn unix_user(user: &str) -> bool {
    user.chars().next().is_some_and(|first| first.is_ascii_lowercase() || first == '_') && user.chars().all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || matches!(character, '_' | '-'))
}
