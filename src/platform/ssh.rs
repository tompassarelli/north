use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

const SOCKET_LIMIT: usize = 107;

pub(crate) fn argv(program: &[OsString], env: &BTreeMap<String, String>, arguments: &[&str]) -> Vec<OsString> {
    let mut argv = program.to_vec();
    let configured = env.get("NORTH_SSH_CONFIG");
    let profile = configured.map_or_else(
        || PathBuf::from(env.get("HOME").map_or("", String::as_str)).join(".config/threads/ssh-config"),
        PathBuf::from,
    );
    if configured.is_some() || profile.is_file() {
        argv.extend(["-F".into(), profile.into_os_string()]);
    }
    if let Some(runtime) = env.get("XDG_RUNTIME_DIR").filter(|runtime| !runtime.is_empty())
        && runtime.starts_with('/')
        && runtime.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_./-".contains(&byte))
        && runtime.len() + "/north-ssh-".len() + 40 + 20 <= SOCKET_LIMIT
    {
        for option in [
            "ControlMaster=auto".to_owned(), format!("ControlPath={runtime}/north-ssh-%C"),
            "ControlPersist=10m".to_owned(), "ServerAliveInterval=5".to_owned(), "ServerAliveCountMax=2".to_owned(),
        ] {
            argv.extend(["-o".into(), option.into()]);
        }
    }
    argv.extend(arguments.iter().map(OsString::from));
    argv
}
