use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

const SOCKET_LIMIT: usize = 107;

pub fn argv(program: &[OsString], env: &BTreeMap<String, String>, arguments: &[&str]) -> Vec<OsString> {
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
        && !chooses_multiplexing(arguments)
        && runtime.starts_with('/')
        && runtime.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_./-".contains(&byte))
        && runtime.len() + "/north-ssh-".len() + 40 + 20 <= SOCKET_LIMIT
    {
        for option in [
            "ControlMaster=no".to_owned(), format!("ControlPath={runtime}/north-ssh-%C"),
            "ServerAliveInterval=5".to_owned(), "ServerAliveCountMax=2".to_owned(),
        ] {
            argv.extend(["-o".into(), option.into()]);
        }
    }
    argv.extend(arguments.iter().map(OsString::from));
    argv
}

fn chooses_multiplexing(arguments: &[&str]) -> bool {
    arguments.iter().enumerate().any(|(at, argument)| {
        let option = match *argument {
            "-o" => arguments.get(at + 1).copied().unwrap_or_default(),
            other => other.strip_prefix("-o").unwrap_or_default(),
        };
        let name = option.trim().split(|c: char| c == '=' || c.is_whitespace()).next().unwrap_or_default().to_ascii_lowercase();
        argument.starts_with("-S") || ["controlmaster", "controlpath", "controlpersist"].contains(&name.as_str())
    })
}

pub fn run(arguments: &[String]) -> u8 {
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let mut out = std::io::stdout().lock();
    for word in argv(&["ssh".into()], &env, &words) {
        if std::io::Write::write_all(&mut out, std::os::unix::ffi::OsStrExt::as_bytes(word.as_os_str())).and_then(|()| std::io::Write::write_all(&mut out, b"\0")).is_err() {
            return 1;
        }
    }
    0
}

#[cfg(test)]
#[path = "ssh_survival_tests.rs"]
mod survival_tests;
