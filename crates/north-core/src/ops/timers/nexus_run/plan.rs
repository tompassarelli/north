use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

pub(super) const USAGE: &str = "usage: north nexus-run [--source DIR] [--cwd PATH] [--install PATH]... [--pull PATH]... [--output-dir DIR] [--timeout-seconds N] -- COMMAND...";

const STOCK_CACHE: &str = "smashcraft-render-assets/stock/65794b789a867a5f745f0cfb3e6d089b197a7bf239fdb43cfd75e5e251015705";

#[derive(Debug, Default, Eq, PartialEq)]
pub(super) struct Options {
    pub(super) source: Option<PathBuf>,
    pub(super) cwd: Option<String>,
    pub(super) install: Vec<String>,
    pub(super) pull: Vec<String>,
    pub(super) output_dir: Option<PathBuf>,
    pub(super) timeout_seconds: u64,
    pub(super) command: Vec<String>,
}

pub(super) enum Parsed {
    Run(Options),
    Help,
}

fn relative(flag: &str, value: &str) -> Result<String, String> {
    let path = Path::new(value);
    if path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
        return Err(format!("argument {flag}: use a path relative to the source tree"));
    }
    Ok(value.to_owned())
}

pub(super) fn parse(arguments: &[String]) -> Result<Parsed, String> {
    let mut options = Options { timeout_seconds: 900, ..Options::default() };
    let mut rest = arguments.iter().peekable();
    while let Some(argument) = rest.peek().map(|argument| argument.as_str()) {
        if argument == "-h" || argument == "--help" {
            return Ok(Parsed::Help);
        }
        if argument == "--" {
            rest.next();
            break;
        }
        if !argument.starts_with("--") {
            break;
        }
        let argument = rest.next().expect("peeked argument").as_str();
        let (flag, inline) = argument.split_once('=').map_or((argument, None), |(flag, value)| (flag, Some(value.to_owned())));
        let value = inline.or_else(|| rest.next().cloned()).ok_or_else(|| format!("argument {flag}: expected one argument"))?;
        match flag {
            "--source" => options.source = Some(PathBuf::from(value)),
            "--cwd" => options.cwd = Some(relative(flag, &value)?),
            "--install" => options.install.push(relative(flag, &value)?),
            "--pull" => options.pull.push(relative(flag, &value)?),
            "--output-dir" => options.output_dir = Some(PathBuf::from(value)),
            "--timeout-seconds" => options.timeout_seconds = value.parse().map_err(|_| format!("argument {flag}: invalid int value: '{value}'"))?,
            other => return Err(format!("unrecognized arguments: {other}")),
        }
    }
    options.command = rest.cloned().collect();
    if options.command.is_empty() || !(1..=3600).contains(&options.timeout_seconds) {
        return Err("provide -- COMMAND and a timeout between 1 and 3600 seconds".to_owned());
    }
    Ok(Parsed::Run(options))
}

pub(super) fn select_files(listing: &[u8], source: &Path) -> Vec<Vec<u8>> {
    let mut names: Vec<&[u8]> = listing.split(|byte| *byte == 0).filter(|name| !name.is_empty()).collect();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .filter(|name| {
            let path = Path::new(OsStr::from_bytes(name));
            let private = path.components().any(|part| {
                let part = part.as_os_str().as_bytes();
                matches!(part, b".git" | b".ssh" | b".env" | b"node_modules") || part.starts_with(b".env.")
            });
            !private && source.join(path).exists()
        })
        .map(<[u8]>::to_vec)
        .collect()
}

pub(super) fn shell_quote(word: &str) -> String {
    let safe = |character: char| character.is_ascii_alphanumeric() || "@%+=:,./-_".contains(character);
    if word.is_empty() {
        "''".to_owned()
    } else if word.chars().all(safe) {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\"'\"'"))
    }
}

pub(super) fn shell_join<S: AsRef<str>>(words: &[S]) -> String {
    words.iter().map(|word| shell_quote(word.as_ref())).collect::<Vec<_>>().join(" ")
}

fn names_multiplexing(arguments: &[&str]) -> bool {
    arguments.iter().enumerate().any(|(index, argument)| {
        let option = if *argument == "-o" {
            arguments.get(index + 1).copied().unwrap_or("")
        } else {
            argument.strip_prefix("-o").unwrap_or("")
        };
        let name = option.split(|c: char| c == '=' || c.is_whitespace()).find(|part| !part.is_empty()).unwrap_or("").to_lowercase();
        argument.starts_with("-S") || matches!(name.as_str(), "controlmaster" | "controlpath" | "controlpersist")
    })
}

pub(super) fn ssh_argv(env: &dyn Fn(&str) -> Option<String>, home: &Path, arguments: &[&str]) -> Vec<String> {
    let mut argv = vec!["ssh".to_owned()];
    let configured = env("NORTH_SSH_CONFIG");
    let profile = configured.as_deref().map_or_else(|| home.join(".config/threads/ssh-config"), PathBuf::from);
    if configured.is_some() || profile.is_file() {
        argv.extend(["-F".to_owned(), profile.to_string_lossy().into_owned()]);
    }
    if let Some(runtime) = env("XDG_RUNTIME_DIR")
        && runtime.starts_with('/')
        && runtime.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_./-".contains(&byte))
        && !names_multiplexing(arguments)
        && runtime.len() + "/north-ssh-".len() + 40 + 20 <= 107
    {
        for option in [
            "ControlMaster=auto".to_owned(),
            format!("ControlPath={runtime}/north-ssh-%C"),
            "ControlPersist=10m".to_owned(),
            "ServerAliveInterval=5".to_owned(),
            "ServerAliveCountMax=2".to_owned(),
        ] {
            argv.extend(["-o".to_owned(), option]);
        }
    }
    argv.extend(arguments.iter().map(|argument| (*argument).to_owned()));
    argv
}

const LAUNCH: &str = r#"pid=$1; timeout=$2; shift 2
printf %s "$$" > "$pid"
skill=$(agents path machine-capacity) || exit $?
exec bun "$(dirname "$skill")/scripts/machine-capacity.mjs" run --class moderate --owner nexus-run --timeout-seconds "$timeout" -- sh -c "$JOB" nexus-job "$@""#;

pub(super) fn job_script() -> String {
    format!(
        r#"root=$1; cwd=$2; count=$3; shift 3
export NEXUS_OUTPUT="$(dirname "$root")/out"
export CLASSIC_EFFECT_CACHE="$HOME/.local/share/{STOCK_CACHE}"
export WC3_STORAGE="$HOME/.local/share/nexus-stock-storage"
cd "$root/$cwd" || exit $?
while [ "$count" -gt 0 ]; do
  (cd "$root/$1" && bun install --frozen-lockfile) || exit $?
  shift; count=$((count - 1))
done
exec "$@""#
    )
}

pub(super) fn launch_words(remote: &str, cwd: &str, options: &Options) -> Vec<String> {
    let launch = format!("JOB={}; export JOB\n{LAUNCH}", shell_quote(&job_script()));
    let mut words: Vec<String> = ["sh", "-c", &launch, "nexus-run"].map(str::to_owned).to_vec();
    words.push(format!("{remote}/run.pid"));
    words.push(options.timeout_seconds.to_string());
    words.push(format!("{remote}/source"));
    words.push(cwd.to_owned());
    words.push(options.install.len().to_string());
    words.extend(options.install.iter().cloned());
    words.extend(options.command.iter().cloned());
    words
}

pub(super) fn stop_words(remote: &str) -> Vec<String> {
    let script = r#"file="$1/run.pid"
[ -f "$file" ] || exit 0
pid=$(cat "$file")
kill -TERM "$pid" 2>/dev/null || exit 0
tries=0
while [ "$tries" -lt 100 ]; do
  kill -0 "$pid" 2>/dev/null || exit 0
  sleep 0.1
  tries=$((tries + 1))
done
echo 'nexus-run: remote wrapper still stopping; preserving workspace' >&2
exit 1"#;
    ["sh", "-c", script, "nexus-run-stop", remote].map(str::to_owned).to_vec()
}
