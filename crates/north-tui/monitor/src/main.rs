mod ansi;
mod budget;
mod collect;
mod frame;
mod rebuild;
mod screen;
mod snapshot;
mod text;

use std::path::PathBuf;
use std::process::ExitCode;

use budget::Budgets;
use toml_edit::DocumentMut;

const USAGE: &str = "usage: north-monitor [--budgets FILE] [--record FILE] [--interval MS]
       north-monitor --once [--ansi] [--size WxH] [--settle SECS]
       north-monitor --replay FILE [--speed N] [--once [--ansi] [--frame N]]
       north-monitor --rebuild FROM_UNIX TO_UNIX [--step SECS] --record FILE";

#[derive(Default)]
pub struct Options {
    pub budgets: Option<PathBuf>,
    pub record: Option<PathBuf>,
    pub replay: Option<PathBuf>,
    pub rebuild: Option<(u64, u64)>,
    pub step: u64,
    pub speed: f64,
    pub interval_ms: u64,
    pub once: bool,
    pub ansi: bool,
    pub size: (u16, u16),
    pub settle_s: u64,
    pub frame: Option<usize>,
}

fn size(value: &str) -> Option<(u16, u16)> {
    let (width, height) = value.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

fn numeric(options: &mut Options, flag: &str, value: &str) -> Option<()> {
    match flag {
        "--step" => options.step = value.parse().ok()?,
        "--speed" => options.speed = value.parse::<f64>().ok().filter(|speed| *speed > 0.0)?,
        "--interval" => options.interval_ms = value.parse::<u64>().ok()?.clamp(1000, 2000),
        "--settle" => options.settle_s = value.parse().ok()?,
        "--frame" => options.frame = Some(value.parse().ok()?),
        "--size" => options.size = size(value)?,
        _ => return None,
    }
    Some(())
}

fn valued(options: &mut Options, flag: &str, value: &str) -> Option<()> {
    match flag {
        "--budgets" => options.budgets = Some(value.into()),
        "--record" => options.record = Some(value.into()),
        "--replay" => options.replay = Some(value.into()),
        _ => return numeric(options, flag, value),
    }
    Some(())
}

fn parse(arguments: &[String]) -> Option<Options> {
    let mut options = Options { step: 30, speed: 1.0, interval_ms: 1500, size: (120, 40), settle_s: 15, ..Options::default() };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--once" => options.once = true,
            "--ansi" => options.ansi = true,
            "--rebuild" => options.rebuild = Some((rest.next()?.parse().ok()?, rest.next()?.parse().ok()?)),
            flag => valued(&mut options, flag, rest.next()?)?,
        }
    }
    Some(options)
}

fn budgets(options: &Options) -> Budgets {
    let default_file = std::env::var_os("HOME").map(|home| PathBuf::from(home).join("code/north/main/harness/holdlist.toml"));
    let chosen = options.budgets.clone().or_else(|| default_file.filter(|path| path.exists()));
    match chosen {
        None => Budgets::defaults(),
        Some(path) => Budgets::load(&path).unwrap_or_else(|error| Budgets { source: format!("built-in defaults ({error})"), ..Budgets::defaults() }),
    }
}

fn configured_hosts() -> Vec<collect::HostTarget> {
    let variable = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let path = north_shared_config::orchestration(&home, variable("AGENTS_ORCHESTRATION").as_deref(), variable("AGENTS_OPERATOR_DIR").as_deref());
    let orchestration = std::fs::read_to_string(path).ok().and_then(|text| text.parse::<DocumentMut>().ok()).unwrap_or_default();
    collect::hosts_in(&orchestration)
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(options) = parse(&arguments) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let budgets = budgets(&options);
    let hosts = configured_hosts();
    let result = match (&options.rebuild, &options.replay) {
        (Some(window), _) => screen::rebuild(&options, *window, &hosts),
        (None, Some(path)) => screen::replay(&options, &budgets, path),
        (None, None) if options.once => screen::once(&options, &budgets, hosts),
        (None, None) => screen::live(&options, &budgets, hosts),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("north-monitor: {error}");
            ExitCode::FAILURE
        }
    }
}
