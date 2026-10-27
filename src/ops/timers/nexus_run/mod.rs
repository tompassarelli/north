mod plan;
#[cfg(test)]
mod tests;

use std::env;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::signal::unix::{SignalKind, signal};

use super::home;
use plan::{Options, Parsed};

const SSH_LIMIT: Duration = Duration::from_secs(90);

#[derive(Default)]
struct Session {
    remote: String,
    running: bool,
}

enum Failure {
    Interrupted,
    Message(String),
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

struct Outcome {
    code: i32,
    stdout: Vec<u8>,
}

pub(crate) async fn run(arguments: &[String]) -> u8 {
    let options = match plan::parse(arguments) {
        Ok(Parsed::Run(options)) => options,
        Ok(Parsed::Help) => {
            println!("{}", plan::USAGE);
            return 0;
        }
        Err(message) => {
            eprintln!("{}\nnexus-run: error: {message}", plan::USAGE);
            return 2;
        }
    };
    let mut session = Session::default();
    let outcome = tokio::select! {
        result = transfer(&options, &mut session) => result,
        () = interrupted() => Err(Failure::Interrupted),
    };
    let cleaned = cleanup(&session).await;
    match (outcome, cleaned) {
        (_, Err(message)) | (Err(Failure::Message(message)), Ok(())) => {
            eprintln!("nexus-run: {message}");
            1
        }
        (Err(Failure::Interrupted), Ok(())) => 130,
        (Ok(code), Ok(())) => u8::try_from(code & 0xff).unwrap_or(1),
    }
}

async fn interrupted() {
    let (Ok(mut terminate), Ok(mut interrupt)) = (signal(SignalKind::terminate()), signal(SignalKind::interrupt())) else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
    }
}

async fn exec(argv: &[String], stdin: Option<&[u8]>, capture: bool, limit: Duration, check: bool) -> Result<Outcome, String> {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).kill_on_drop(true);
    command.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::inherit() });
    command.stdout(if capture { Stdio::piped() } else { Stdio::inherit() });
    let mut child = command.spawn().map_err(|error| format!("{}: {error}", argv[0]))?;
    let mut pipe = child.stdin.take();
    let feed = async {
        if let (Some(pipe), Some(bytes)) = (pipe.as_mut(), stdin) {
            let _ = pipe.write_all(bytes).await;
        }
        drop(pipe.take());
    };
    let wait = async { tokio::join!(feed, child.wait_with_output()).1 };
    let output = tokio::time::timeout(limit, wait)
        .await
        .map_err(|_| format!("Command '{}' timed out after {} seconds", argv.join(" "), limit.as_secs()))?
        .map_err(|error| error.to_string())?;
    let code = output.status.code().unwrap_or(1);
    if check && code != 0 {
        return Err(format!("Command '{}' returned non-zero exit status {code}.", argv.join(" ")));
    }
    Ok(Outcome { code, stdout: output.stdout })
}

fn real_env(name: &str) -> Option<String> {
    env::var(name).ok()
}

fn ssh_command(words: &[String]) -> Vec<String> {
    plan::ssh_argv(&real_env, &home(), &["-o", "BatchMode=yes", "nexus", &plan::shell_join(words)])
}

async fn ssh(words: &[&str], check: bool) -> Result<Outcome, String> {
    let owned: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
    exec(&ssh_command(&owned), None, false, SSH_LIMIT, check).await
}

async fn copy(route: &str, source: &str, destination: &str, extra: &[String], stdin: Option<&[u8]>, limit: u64) -> Result<(), String> {
    let mut argv: Vec<String> = ["rsync", "-a", "--protect-args", "-e", route].map(str::to_owned).to_vec();
    argv.extend(extra.iter().cloned());
    argv.extend([source.to_owned(), destination.to_owned()]);
    exec(&argv, stdin, false, Duration::from_secs(limit + 30), true).await.map(drop)
}

fn job_id() -> Result<String, String> {
    let mut raw = [0u8; 16];
    let mut urandom = std::fs::File::open("/dev/urandom").map_err(|error| error.to_string())?;
    std::io::Read::read_exact(&mut urandom, &mut raw).map_err(|error| error.to_string())?;
    Ok(raw.iter().map(|byte| format!("{byte:02x}")).collect())
}

async fn locate(options: &Options) -> Result<(PathBuf, String), String> {
    let origin = std::fs::canonicalize(env::current_dir().map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let source = match &options.source {
        Some(path) => path.clone(),
        None => {
            let top = crate::platform::git::run(&origin, &["rev-parse", "--show-toplevel"]).map_err(|error| error.to_string())?;
            if !top.status.success() {
                return Err("git rev-parse --show-toplevel failed".to_owned());
            }
            PathBuf::from(String::from_utf8_lossy(&top.stdout).trim())
        }
    };
    let source = std::fs::canonicalize(&source).map_err(|error| format!("{}: {error}", source.display()))?;
    let cwd = match &options.cwd {
        Some(cwd) => cwd.clone(),
        None => {
            let inside = origin.strip_prefix(&source).map_err(|_| format!("'{}' is not in the subpath of '{}'", origin.display(), source.display()))?;
            if inside.as_os_str().is_empty() { ".".to_owned() } else { inside.to_string_lossy().into_owned() }
        }
    };
    Ok((source, cwd))
}

pub(super) fn baseline_for(source: &Path, home: &Path, remote_home: &str) -> String {
    match source.strip_prefix(home.join("code")).ok().and_then(|inside| inside.components().next()) {
        Some(project) => format!("{remote_home}/code/{}/main", project.as_os_str().to_string_lossy()),
        None => source.to_string_lossy().into_owned(),
    }
}

async fn transfer(options: &Options, session: &mut Session) -> Result<i32, Failure> {
    let (source, cwd) = locate(options).await?;
    let job = job_id()?;
    let home_output = exec(&plan::ssh_argv(&real_env, &home(), &["-o", "BatchMode=yes", "nexus", "printf %s \"$HOME\""]), None, true, SSH_LIMIT, true).await?;
    let remote_home = String::from_utf8_lossy(&home_output.stdout).into_owned();
    let remote = format!("{remote_home}/.cache/nexus-run/{job}");
    let output = std::path::absolute(options.output_dir.clone().unwrap_or_else(|| home().join(".cache/nexus-run").join(&job))).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&output).map_err(|error| error.to_string())?;
    let route = plan::shell_join(&plan::ssh_argv(&real_env, &home(), &["-o", "BatchMode=yes"]));
    let listing = crate::platform::git::stdout(&source, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"], |_| "git ls-files failed".to_owned())?;
    let baseline = baseline_for(&source, &home(), &remote_home);
    let reuse = if ssh(&["test", "-d", &baseline], false).await?.code == 0 { vec!["--checksum".to_owned(), format!("--copy-dest={baseline}")] } else { Vec::new() };
    let selected = plan::select_files(&listing, &source);
    session.remote.clone_from(&remote);
    ssh(&["mkdir", "-p", &format!("{remote}/source"), &format!("{remote}/out")], true).await?;
    let mut copy_options = reuse;
    copy_options.extend(["--from0".to_owned(), "--files-from=-".to_owned()]);
    let names: Vec<u8> = selected.iter().flat_map(|name| name.iter().copied().chain([0])).collect();
    copy(&route, &format!("{}/", source.display()), &format!("nexus:{remote}/source/"), &copy_options, Some(&names), options.timeout_seconds).await?;
    ssh(&["git", "-C", &format!("{remote}/source"), "init", "-q"], true).await?;
    eprintln!("nexus-run: remote={remote} outputs={}", output.display());
    session.running = true;
    let limit = Duration::from_secs(options.timeout_seconds + 30);
    let launched = exec(&ssh_command(&plan::launch_words(&remote, &cwd, options)), None, false, limit, false).await?;
    session.running = launched.code == 255;
    let timeout = options.timeout_seconds;
    copy(&route, &format!("nexus:{remote}/out/"), &format!("{}/", output.display()), &[], None, timeout).await?;
    for path in &options.pull {
        let target = output.join("source").join(path);
        std::fs::create_dir_all(target.parent().unwrap_or(&output)).map_err(|error| error.to_string())?;
        copy(&route, &format!("nexus:{remote}/source/{path}"), &target.to_string_lossy(), &[], None, timeout).await?;
    }
    Ok(launched.code)
}

async fn cleanup(session: &Session) -> Result<(), String> {
    if !session.remote.contains("/.cache/nexus-run/") {
        return Ok(());
    }
    if session.running {
        let stop = plan::stop_words(&session.remote);
        exec(&ssh_command(&stop), None, false, SSH_LIMIT, true).await?;
    }
    ssh(&["rm", "-rf", "--", &session.remote], true).await.map(drop)
}
