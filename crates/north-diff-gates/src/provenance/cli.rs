use super::check::check;
use super::debt::debt;
use std::path::Path;

const HELP: &str = r#"usage: north check provenance [-h] {check,debt} ...

provenance: refuse new steering without provenance, and list Target-debt.

positional arguments:
  {check,debt}
    check             list new gating bars, hooks and policy lines without a
                      Provenance record
    debt              list Target-debt trailers on origin/main not named by a
                      Target-paid

options:
  -h, --help          show this help message and exit
"#;
const HELP_CHECK: &str = r#"usage: north check provenance check [-h] base head

positional arguments:
  base
  head

options:
  -h, --help  show this help message and exit
"#;
const HELP_DEBT: &str = r#"usage: north check provenance debt [-h] [--due] [--repo REPO]

options:
  -h, --help   show this help message and exit
  --due        only rows whose revisit date passed or issue closed, and every
               event text
  --repo REPO  repository path (default ~/code/*/main)
"#;
enum Invocation {
    Check { base: String, head: String },
    Debt { paths: Vec<String>, only_due: bool },
}

#[derive(Debug)]
enum Exit {
    Help(&'static str),
    Usage { command: String, message: String },
}

impl Exit {
    fn usage(command: &str, message: impl Into<String>) -> Self {
        Exit::Usage { command: command.to_owned(), message: message.into() }
    }

    fn code(&self) -> u8 {
        match self {
            Exit::Help(_) => 0,
            Exit::Usage { .. } => 2,
        }
    }

    fn stdout(&self) -> &str {
        match self {
            Exit::Help(text) => text,
            Exit::Usage { .. } => "",
        }
    }

    fn stderr(&self) -> String {
        let Exit::Usage { command, message } = self else {
            return String::new();
        };
        let help = match command.as_str() {
            "check" => HELP_CHECK,
            "debt" => HELP_DEBT,
            _ => HELP,
        };
        let name = if command.is_empty() { "provenance".to_owned() } else { format!("provenance {command}") };
        format!("{}\n{name}: error: {message}\n", help.lines().next().unwrap())
    }
}

fn is_help(key: &str) -> bool {
    key == "-h" || (key.len() > 2 && "--help".starts_with(key))
}

fn parse(args: &[String]) -> Result<Invocation, Exit> {
    let mut start = 0;
    while start < args.len() && args[start].starts_with('-') && args[start] != "--" {
        let (key, value) = args[start]
            .split_once('=')
            .map_or((args[start].as_str(), None), |(k, v)| (k, Some(v)));
        if is_help(key) {
            if let Some(value) = value {
                return Err(Exit::usage("", format!("argument -h/--help: ignored explicit argument '{value}'")));
            }
            return Err(Exit::Help(HELP));
        }
        start += 1;
    }
    let leading = &args[..start];
    let args = &args[start..];
    let Some(command) = args.first() else {
        return Err(Exit::usage("", "the following arguments are required: command"));
    };
    if command == "check" {
        let mut positional = Vec::new();
        let mut unknown = Vec::new();
        let mut options = true;
        for arg in &args[1..] {
            let (key, value) = arg
                .split_once('=')
                .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
            if options && is_help(key) {
                if let Some(value) = value {
                    return Err(Exit::usage("check", format!("argument -h/--help: ignored explicit argument '{value}'")));
                }
                return Err(Exit::Help(HELP_CHECK));
            } else if options && arg == "--" {
                options = false;
            } else if options && arg.starts_with('-') && !re!(r"\A-\d+$|\A-\d*\.\d+$").is_match(arg)
            {
                unknown.push(arg.as_str());
            } else if positional.len() < 2 {
                positional.push(arg);
            } else {
                unknown.push(arg.as_str());
            }
        }
        if positional.len() < 2 {
            return Err(Exit::usage(
                "check",
                if positional.is_empty() {
                    "the following arguments are required: base, head"
                } else {
                    "the following arguments are required: head"
                },
            ));
        }
        unknown.splice(0..0, leading.iter().map(String::as_str));
        if !unknown.is_empty() {
            return Err(Exit::usage("", format!("unrecognized arguments: {}", unknown.join(" "))));
        }
        return Ok(Invocation::Check { base: positional[0].clone(), head: positional[1].clone() });
    }
    if command != "debt" {
        return Err(Exit::usage(
            "",
            format!("argument command: invalid choice: '{command}' (choose from 'check', 'debt')"),
        ));
    }
    let mut paths = Vec::new();
    let mut only_due = false;
    let mut unknown: Vec<_> = leading.iter().map(String::as_str).collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--" {
            unknown.extend(args[i..].iter().map(String::as_str));
            break;
        }
        let (key, inline) = args[i]
            .split_once('=')
            .map_or((args[i].as_str(), None), |(k, v)| (k, Some(v)));
        if is_help(key) {
            if let Some(value) = inline {
                return Err(Exit::usage(command, format!("argument -h/--help: ignored explicit argument '{value}'")));
            }
            return Err(Exit::Help(HELP_DEBT));
        }
        let options: &[&str] = &["--due", "--repo"];
        let candidates: Vec<_> = options
            .iter()
            .copied()
            .filter(|opt| key.starts_with("--") && opt.starts_with(key))
            .collect();
        if candidates.len() > 1 {
            return Err(Exit::usage(command, format!("ambiguous option: {key} could match {}", candidates.join(", "))));
        }
        let key = candidates.first().copied().unwrap_or(key);
        match key {
            "--due" => {
                if let Some(value) = inline {
                    return Err(Exit::usage(command, format!("argument --due: ignored explicit argument '{value}'")));
                }
                only_due = true;
            }
            "--repo" => {
                let value = if let Some(v) = inline {
                    v
                } else {
                    i += 1;
                    if i >= args.len()
                        || (args[i].starts_with('-')
                            && !re!(r"\A-\d+$|\A-\d*\.\d+$").is_match(&args[i]))
                    {
                        return Err(Exit::usage(command, format!("argument {key}: expected one argument")));
                    }
                    &args[i]
                };
                paths.push(value.to_owned());
            }
            _ => {
                unknown.push(args[i].as_str());
            }
        }
        i += 1;
    }
    if !unknown.is_empty() {
        return Err(Exit::usage("", format!("unrecognized arguments: {}", unknown.join(" "))));
    }
    Ok(Invocation::Debt { paths, only_due })
}

pub fn run(args: &[String]) -> u8 {
    let invocation = match parse(args) {
        Ok(invocation) => invocation,
        Err(exit) => {
            print!("{}", exit.stdout());
            eprint!("{}", exit.stderr());
            return exit.code();
        }
    };
    let result = match invocation {
        Invocation::Check { base, head } => {
            return match check(Path::new("."), &base, &head) {
                Ok(c) => c,
                Err(e) => {
                    eprint!("{e}");
                    1
                }
            };
        }
        Invocation::Debt { paths, only_due } => debt(&paths, only_due),
    };
    if let Err(error) = result {
        eprintln!("provenance: {error}");
        return 1;
    }
    0
}
