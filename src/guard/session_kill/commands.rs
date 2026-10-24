use regex::Regex;
use std::sync::LazyLock;

const SESSION_PROCESSES: &[&str] = &["niri", "niri-session"];
const SESSION_UNITS: &[&str] = &["niri", "niri.service", "graphical-session.target"];
const LOGINCTL_VERBS: &[&str] = &[
    "terminate-user",
    "kill-user",
    "terminate-session",
    "kill-session",
    "terminate-seat",
];
const PKILL_VALUE_FLAGS: &[&str] = &[
    "-u",
    "-U",
    "--uid",
    "--euid",
    "-g",
    "-G",
    "--group",
    "-s",
    "--signal",
    "-P",
    "--parent",
    "-t",
    "--terminal",
    "-F",
    "--pidfile",
    "-d",
    "--delay",
    "-o",
    "-n",
];
const SSH_VALUE_FLAGS: &str = "BbcDEeFIiJLlmOoPpQRSWw";
const SYSTEMD_VALUE_FLAGS: &[&str] = &[
    "-H",
    "--host",
    "-M",
    "--machine",
    "-t",
    "--type",
    "-p",
    "--property",
    "-P",
    "-s",
    "--signal",
    "-n",
    "--lines",
    "-o",
    "--output",
    "--state",
    "--kill-whom",
    "--kill-who",
    "--kill-value",
    "--root",
    "--image",
    "--job-mode",
    "--what",
    "--timestamp",
    "--message",
    "--when",
    "--preset-mode",
    "--drop-in",
    "--check-inhibitors",
    "--boot-loader-entry",
    "--boot-loader-menu",
    "--reboot-argument",
];
pub(super) const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh"];
const SHELL_VALUE_FLAGS: &[&str] = &["--rcfile", "--init-file"];

static SIGNAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-(?:[0-9]+|[A-Za-z]+)$").unwrap());

pub(super) fn kill(args: &[String]) -> Option<&'static str> {
    let mut operands = Vec::new();
    let mut index = 0;
    let mut options = true;
    while index < args.len() {
        let arg = args[index].as_str();
        if options {
            match arg {
                "--" => {
                    options = false;
                    index += 1;
                    continue;
                }
                "-l" | "-L" => return None,
                "-s" | "-n" | "--signal" if index + 1 < args.len() => {
                    index += 2;
                    continue;
                }
                _ if index == 0 && SIGNAL.is_match(arg) && (arg != "-1" || args.len() > 1) => {
                    index += 1;
                    continue;
                }
                _ => {}
            }
        }
        operands.push(arg);
        index += 1;
    }
    operands.contains(&"-1").then_some("broadcast kill -1")
}

pub(super) fn pkill(args: &[String]) -> Option<&'static str> {
    let mut user = false;
    let mut operands = Vec::new();
    let mut skip = false;
    for arg in args {
        if std::mem::take(&mut skip) {
            continue;
        }
        if matches!(arg.as_str(), "-u" | "-U")
            || arg.starts_with("--uid")
            || arg.starts_with("--euid")
        {
            user = true;
            skip = PKILL_VALUE_FLAGS.contains(&arg.as_str());
            continue;
        }
        if PKILL_VALUE_FLAGS.contains(&arg.as_str()) {
            skip = true;
            continue;
        }
        if !arg.starts_with('-') {
            operands.push(arg.as_str());
        }
    }
    if user && operands.is_empty() {
        Some("user-wide pkill sweep")
    } else if operands.iter().any(|o| SESSION_PROCESSES.contains(o)) {
        Some("compositor kill")
    } else {
        None
    }
}

pub(super) fn killall(args: &[String]) -> Option<&'static str> {
    if args.iter().any(|a| a == "-u" || a == "--user") {
        Some("user-wide killall sweep")
    } else if args.iter().any(|a| SESSION_PROCESSES.contains(&a.as_str())) {
        Some("compositor kill")
    } else {
        None
    }
}

pub(super) struct Systemd<'a> {
    user: bool,
    pub(super) host: Option<String>,
    operands: Vec<&'a str>,
}

impl<'a> Systemd<'a> {
    pub(super) fn parse(args: &'a [String]) -> Self {
        let mut call = Systemd {
            user: false,
            host: None,
            operands: Vec::new(),
        };
        let mut iter = args.iter().map(String::as_str);
        while let Some(arg) = iter.next() {
            let value = if SYSTEMD_VALUE_FLAGS.contains(&arg) {
                iter.next()
            } else {
                None
            };
            match arg {
                "--user" => call.user = true,
                "-H" | "--host" => call.host = value.map(remote_host),
                _ if arg.starts_with("--host=") => call.host = Some(remote_host(&arg[7..])),
                _ if arg.starts_with("-H") && arg.len() > 2 => {
                    call.host = Some(remote_host(&arg[2..]))
                }
                _ if arg.starts_with('-') => {}
                _ => call.operands.push(arg),
            }
        }
        call
    }

    pub(super) fn loginctl(&self) -> Option<&'static str> {
        LOGINCTL_VERBS
            .contains(self.operands.first()?)
            .then_some("login-session teardown")
    }

    pub(super) fn systemctl(&self) -> Option<&'static str> {
        let (verb, units) = self.operands.split_first()?;
        match *verb {
            "exit" if self.user => Some("user-manager exit"),
            "isolate" if units.iter().any(|u| u.starts_with("exit")) => Some("user-manager exit"),
            "stop"
            | "kill"
            | "restart"
            | "try-restart"
            | "condrestart"
            | "reload-or-restart"
            | "try-reload-or-restart"
            | "force-reload" => {
                if units.iter().any(|unit| user_manager_unit(unit)) {
                    Some("user-manager teardown")
                } else if self.user && units.iter().any(|u| SESSION_UNITS.contains(u)) {
                    Some("compositor teardown")
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

fn user_manager_unit(unit: &str) -> bool {
    unit.starts_with("user@")
        || unit.starts_with("user-runtime-dir@")
        || unit == "user.slice"
        || unit.starts_with("user-") && unit.ends_with(".slice")
}

fn remote_host(value: &str) -> String {
    let host = value.rsplit('@').next().unwrap_or(value);
    host.split(':').next().unwrap_or(host).to_owned()
}

pub(super) fn ssh(args: &[String]) -> Option<(String, Option<String>)> {
    let mut destination = None;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        if arg == "--" {
            if destination.is_none() {
                destination = args.get(index).cloned();
                index += 1;
            }
            break;
        }
        if let Some(flags) = arg.strip_prefix('-').filter(|flags| !flags.is_empty()) {
            if let Some(at) = flags.find(|c| SSH_VALUE_FLAGS.contains(c))
                && at + 1 == flags.len()
            {
                index += 1;
            }
            continue;
        }
        if destination.is_some() {
            index -= 1;
            break;
        }
        destination = Some(arg.to_owned());
    }
    let remote = args
        .get(index..)
        .filter(|rest| !rest.is_empty())
        .map(|rest| rest.join(" "));
    Some((destination?, remote))
}

pub(super) enum Script<'a> {
    Inline(&'a str),
    Stdin,
    File,
}

pub(super) fn shell_script(args: &[String]) -> Script<'_> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-" => return Script::Stdin,
            "--" => return iter.next().map_or(Script::Stdin, |_| Script::File),
            flag if SHELL_VALUE_FLAGS.contains(&flag) => {
                iter.next();
                continue;
            }
            flag if flag.starts_with("--") => continue,
            _ => {}
        }
        let Some(flags) = arg
            .strip_prefix(['-', '+'])
            .filter(|flags| !flags.is_empty())
        else {
            return Script::File;
        };
        if flags.contains('c') {
            return iter
                .next()
                .map_or(Script::File, |script| Script::Inline(script));
        }
        if flags.contains('s') {
            return Script::Stdin;
        }
        if flags.ends_with(['o', 'O']) {
            iter.next();
        }
    }
    Script::Stdin
}
