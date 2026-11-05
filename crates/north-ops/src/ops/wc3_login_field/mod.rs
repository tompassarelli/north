#[cfg(test)]
mod tests;

use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

const USAGE: &str = "Usage: north wc3-login-field PRIVATE_RUN a|b|c|d username|password [--owner-authorized]";
const LOGIN_TITLES: [&str; 3] = ["Battle.net", "Battle.net Login", "Warcraft III"];

pub struct Failure(pub String, pub u8);

pub struct Request {
    pub run: PathBuf,
    pub account: String,
    pub field: String,
    pub owner_authorized: bool,
}

pub trait Desktop {
    fn active_window(&self, display: &str, xauthority: &str) -> Result<(String, String), Failure>;
    fn secrets(&self) -> Result<Value, Failure>;
    fn type_text(&self, run: &Path, text: &str) -> Result<(), Failure>;
}

pub fn parse(arguments: &[String]) -> Result<Request, Failure> {
    let (authorized, words) = match arguments {
        [first, second, third, flag] if flag == "--owner-authorized" => (true, [first, second, third]),
        [first, second, third] => (false, [first, second, third]),
        _ => return Err(Failure(USAGE.to_owned(), 2)),
    };
    let [run, account, field] = words;
    if !matches!(account.as_str(), "a" | "b" | "c" | "d") || !matches!(field.as_str(), "username" | "password") {
        return Err(Failure(USAGE.to_owned(), 2));
    }
    Ok(Request { run: PathBuf::from(run), account: account.clone(), field: field.clone(), owner_authorized: authorized })
}

fn recorded(run: &Path, name: &str) -> Result<String, Failure> {
    fs::read_to_string(run.join(name)).map(|text| text.trim_end_matches('\n').to_owned()).map_err(|_| Failure(String::new(), 1))
}

pub fn deliver(request: &Request, private_root: &Path, desktop: &dyn Desktop) -> Result<&'static str, Failure> {
    let under_root = request.run.parent() == Some(private_root) && request.run.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("private-desktop."));
    if !under_root {
        return Err(Failure("Private desktop required.".to_owned(), 1));
    }
    let socket = fs::metadata(request.run.join("runtime/wayland-0")).is_ok_and(|meta| meta.file_type().is_socket());
    if !socket || !request.run.join("display").is_file() || !request.run.join("xauthority").is_file() {
        return Err(Failure(String::new(), 1));
    }
    let display = recorded(&request.run, "display")?;
    let xauthority = recorded(&request.run, "xauthority")?;
    if display.is_empty() {
        return Err(Failure(String::new(), 1));
    }
    let (window, title) = desktop.active_window(&display, &xauthority)?;
    if !LOGIN_TITLES.contains(&title.as_str()) {
        return Err(Failure("Login window is not active.".to_owned(), 1));
    }
    let secrets = desktop.secrets()?;
    let account = &secrets[request.account.as_str()];
    if !request.owner_authorized && account["owner_reserved_unencrypted"] == true {
        return Err(Failure(format!("Refused: account {} is the owner's personal account and needs the owner's explicit authorization this session (--owner-authorized).", request.account), 3));
    }
    let value = account[request.field.as_str()].as_str().ok_or_else(|| Failure("Missing account field".to_owned(), 1))?;
    desktop.type_text(&request.run, value)?;
    if desktop.active_window(&display, &xauthority)?.0 != window {
        return Err(Failure("Focus changed during delivery; inspect privately before proceeding.".to_owned(), 1));
    }
    Ok("Field delivered; verify the login result privately.")
}

struct Real;

fn present(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

fn xdotool(display: &str, xauthority: &str, arguments: &[&str]) -> Result<String, Failure> {
    let output = Command::new("xdotool").args(arguments).env("DISPLAY", display).env("XAUTHORITY", xauthority).output();
    output.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned()).ok_or_else(|| Failure("Login window is not active.".to_owned(), 1))
}

impl Desktop for Real {
    fn active_window(&self, display: &str, xauthority: &str) -> Result<(String, String), Failure> {
        if let Some(missing) = ["xdotool", "yq"].into_iter().find(|program| !present(program)) {
            return Err(Failure(format!("Missing dependency: {missing}"), 1));
        }
        let window = xdotool(display, xauthority, &["getactivewindow"])?;
        let title = xdotool(display, xauthority, &["getwindowname", &window])?;
        Ok((window, title))
    }


    fn secrets(&self) -> Result<Value, Failure> {
        let file = secret_file();
        if fs::File::open(&file).is_err() {
            return Err(Failure("Account secret unavailable.".to_owned(), 1));
        }
        let output = Command::new("yq").args(["-o", "json", "."]).arg(&file).stderr(Stdio::null()).output();
        output.ok().filter(|output| output.status.success()).and_then(|output| serde_json::from_slice(&output.stdout).ok()).ok_or_else(|| Failure("Account secret unavailable.".to_owned(), 1))
    }

    fn type_text(&self, run: &Path, text: &str) -> Result<(), Failure> {
        crate::ops::private_desktop::type_into(&run.to_string_lossy(), text).map(|_| ()).map_err(|error| Failure(format!("private-desktop: {error}"), 1))
    }
}

fn secret_file() -> PathBuf {
    PathBuf::from("/run/secrets/bnet")
}

pub fn run(arguments: &[String]) -> u8 {
    let result = parse(arguments).and_then(|request| deliver(&request, &PathBuf::from(format!("/run/user/{}", rustix::process::getuid().as_raw())), &Real));
    match result {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(Failure(message, code)) => {
            if !message.is_empty() {
                eprintln!("{message}");
            }
            code
        }
    }
}
