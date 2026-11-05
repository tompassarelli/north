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

const PGREP_VALUE_SHORT: &str = "uUgGsPtFd";
const PGREP_VALUE_LONG: &[&str] = &[
    "--uid",
    "--euid",
    "--group",
    "--signal",
    "--parent",
    "--terminal",
    "--pidfile",
    "--delay",
    "--session",
    "--pgroup",
    "--runstates",
    "--nslist",
    "--ns",
    "--env",
];

static SIGNAL_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-(?:[0-9]+|[A-Z][A-Z0-9]+)$").unwrap());

pub(super) fn full_pattern(args: &[String]) -> Option<(String, bool)> {
    let (mut full, mut exempt, mut insensitive) = (false, false, false);
    let mut pattern = None;
    let mut iter = args.iter().map(String::as_str);
    while let Some(arg) = iter.next() {
        if arg == "--" {
            pattern = pattern.or(iter.next());
            break;
        }
        if let Some(long) = arg.strip_prefix("--") {
            match long {
                "full" => full = true,
                "exact" | "ignore-ancestors" => exempt = true,
                "ignore-case" => insensitive = true,
                _ if PGREP_VALUE_LONG.contains(&arg) => {
                    iter.next();
                }
                _ => {}
            }
        } else if arg.len() > 1 && arg.starts_with('-') {
            if SIGNAL_NAME.is_match(arg) {
                continue;
            }
            for (at, flag) in arg[1..].char_indices() {
                match flag {
                    'f' => full = true,
                    'x' | 'A' => exempt = true,
                    'i' => insensitive = true,
                    flag if PGREP_VALUE_SHORT.contains(flag) => {
                        if at + 2 == arg.len() {
                            iter.next();
                        }
                        break;
                    }
                    _ => {}
                }
            }
        } else if pattern.is_none() {
            pattern = Some(arg);
        }
    }
    (full && !exempt).then_some(())?;
    Some((pattern?.to_owned(), insensitive))
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

pub(super) const TERMINAL_UNIT: &str = "terminal-scope teardown";
pub(super) const OWN_UNIT: &str = "own-session unit teardown";
pub(super) const DYNAMIC_UNIT: &str = "unseen unit operand teardown";
pub(super) const UNIT_SHAPES: &[&str] = &[TERMINAL_UNIT, OWN_UNIT, DYNAMIC_UNIT];
const TEARDOWN_VERBS: &[&str] = &[
    "stop",
    "kill",
    "restart",
    "try-restart",
    "condrestart",
    "reload-or-restart",
    "try-reload-or-restart",
    "force-reload",
];

pub(super) struct Systemd<'a> {
    user: bool,
    pub(super) host: Option<String>,
    operands: Vec<&'a str>,
}

impl<'a> Systemd<'a> {
    pub(super) fn parse(args: &'a [String]) -> Self {
        Self::with_values(args, SYSTEMD_VALUE_FLAGS)
    }

    pub(super) fn flat(args: &'a [String]) -> Self {
        Self::with_values(args, &[])
    }

    fn with_values(args: &'a [String], value_flags: &[&str]) -> Self {
        let mut call = Systemd {
            user: false,
            host: None,
            operands: Vec::new(),
        };
        let mut iter = args.iter().map(String::as_str);
        while let Some(arg) = iter.next() {
            let value = if value_flags.contains(&arg) {
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

    pub(super) fn teardown_verb(&self) -> bool {
        self.operands
            .first()
            .is_some_and(|verb| TEARDOWN_VERBS.contains(verb))
    }

    pub(super) fn systemctl(&self, own: Option<&[String]>) -> Option<&'static str> {
        let (verb, units) = self.operands.split_first()?;
        match *verb {
            "exit" if self.user => Some("user-manager exit"),
            "isolate" if units.iter().any(|u| u.starts_with("exit")) => Some("user-manager exit"),
            verb if TEARDOWN_VERBS.contains(&verb) => {
                if units.iter().any(|unit| user_manager_unit(unit)) {
                    Some("user-manager teardown")
                } else if self.user && units.iter().any(|u| SESSION_UNITS.contains(u)) {
                    Some("compositor teardown")
                } else {
                    units.iter().find_map(|unit| unit_shape(unit, own?))
                }
            }
            _ => None,
        }
    }
}

fn unit_shape(unit: &str, own: &[String]) -> Option<&'static str> {
    if unit.contains(['$', '`']) {
        Some(DYNAMIC_UNIT)
    } else if terminal_unit(unit) {
        Some(TERMINAL_UNIT)
    } else if own.iter().any(|own| names(unit, own)) {
        Some(OWN_UNIT)
    } else {
        None
    }
}

fn terminal_unit(unit: &str) -> bool {
    let glob = unit.contains(['*', '?', '[']);
    let kind = unit.ends_with(".scope") || unit.ends_with(".service");
    unit.starts_with("app-") && (kind || glob) || glob && unit.ends_with(".scope")
}

fn names(unit: &str, own: &str) -> bool {
    unit == own || !unit.contains('.') && own.strip_suffix(".service") == Some(unit)
}

pub(super) fn cgroup_units(cgroup: &str) -> Vec<String> {
    cgroup
        .lines()
        .filter_map(|line| line.splitn(3, ':').nth(2))
        .flat_map(|path| path.split('/'))
        .filter(|unit| [".slice", ".scope", ".service"].iter().any(|kind| unit.ends_with(kind)))
        .map(str::to_owned)
        .collect()
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

const PKILL_SCOPES: &[&str] = &["-F", "--pidfile", "--cgroup", "-P", "--parent"];
const KILLALL_VALUE_FLAGS: &[&str] = &["-s", "--signal", "-o", "--older-than", "-y", "--younger-than", "-n", "--ns", "-Z", "--context"];

pub(super) fn cross_session(tool: &str, args: &[String]) -> bool {
    let (values, scopes): (&[&str], &[&str]) = match tool {
        "pkill" => (PKILL_VALUE_FLAGS, PKILL_SCOPES),
        _ => (KILLALL_VALUE_FLAGS, &[]),
    };
    let mut named = false;
    let mut iter = args.iter().map(String::as_str);
    while let Some(arg) = iter.next() {
        let flag = arg.split('=').next().unwrap_or(arg);
        if scopes.contains(&flag) {
            return false;
        }
        if matches!(arg, "-l" | "--list" | "-V" | "--version") && tool == "killall" {
            return false;
        }
        if arg == "--" {
            named |= iter.next().is_some();
            break;
        }
        if values.contains(&arg) || arg == "--cgroup" {
            iter.next();
        } else if !arg.starts_with('-') {
            named = true;
        }
    }
    named
}
