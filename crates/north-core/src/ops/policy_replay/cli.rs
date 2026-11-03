use std::path::PathBuf;

use super::point::{PromoteArgs, promote};
use super::run::{Pick, state_dir};
use super::sweep::{Arms, Common, Sweep, arms, sweep};
use crate::error::NorthError;

const USAGE: &str = "usage: north policy-replay promote SCENARIO.json... [--candidate PATH] [--operating-point PATH] [--accept-drop REASON]
       north policy-replay run SCENARIO.json... --arm ARM... --out RESULTS.jsonl [--trials N] [--jobs N] [--policy PATH] [--model M] [--effort E] [--advisor M]
       north policy-replay sweep SCENARIO.json... --variants VARIANTS.toml [--variant NAME]... [--candidate PATH] [--provisional] [--trials N] [--jobs N] [--policy PATH] [--out PATH]";

#[derive(Default)]
struct Options {
    scenarios: Vec<PathBuf>,
    candidate: Option<PathBuf>,
    operating_point: Option<PathBuf>,
    accept_drop: Option<String>,
    arms: Vec<String>,
    variants: Option<PathBuf>,
    only: Vec<String>,
    provisional: bool,
    trials: Option<usize>,
    jobs: Option<usize>,
    policy: Option<PathBuf>,
    out: Option<PathBuf>,
    pick: Pick,
}

fn parse(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let mut value = |flag: &str| rest.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        let mut count = |flag: &str| value(flag).and_then(|text| text.parse::<usize>().map_err(|_| format!("{flag} needs a whole number")));
        match argument.as_str() {
            "--candidate" => options.candidate = Some(PathBuf::from(value("--candidate")?)),
            "--operating-point" => options.operating_point = Some(PathBuf::from(value("--operating-point")?)),
            "--accept-drop" => options.accept_drop = Some(value("--accept-drop")?),
            "--arm" => options.arms.push(value("--arm")?),
            "--variants" => options.variants = Some(PathBuf::from(value("--variants")?)),
            "--variant" => options.only.push(value("--variant")?),
            "--provisional" => options.provisional = true,
            "--trials" => options.trials = Some(count("--trials")?),
            "--jobs" => options.jobs = Some(count("--jobs")?),
            "--policy" => options.policy = Some(PathBuf::from(value("--policy")?)),
            "--out" => options.out = Some(PathBuf::from(value("--out")?)),
            "--model" => options.pick.model = Some(value("--model")?),
            "--effort" => options.pick.effort = Some(value("--effort")?),
            "--advisor" => options.pick.advisor = Some(value("--advisor")?),
            flag if flag.starts_with("--") => return Err(format!("unrecognized arguments: {flag}")),
            file => options.scenarios.push(PathBuf::from(file)),
        }
    }
    if options.scenarios.is_empty() {
        return Err("the following arguments are required: scenarios".into());
    }
    Ok(options)
}

fn default_candidate() -> PathBuf {
    state_dir().join("operating-point-candidate.toml")
}

fn finish(result: Result<u8, NorthError>) -> u8 {
    match result {
        Ok(code) => code,
        Err(NorthError::Refused(message) | NorthError::Usage(message)) => {
            eprintln!("{message}");
            1
        }
        Err(error) => {
            eprintln!("policy-replay: {error}");
            1
        }
    }
}

fn promoted(options: &Options) -> Result<u8, NorthError> {
    let candidate = options.candidate.clone().unwrap_or_else(default_candidate);
    let args = PromoteArgs {
        candidate: &candidate,
        scenarios: &options.scenarios,
        operating_point: options.operating_point.as_deref(),
        accept_drop: options.accept_drop.as_deref(),
    };
    println!("{}", promote(&args)?);
    Ok(0)
}

fn common(options: Options) -> Common {
    Common {
        scenarios: options.scenarios,
        trials: options.trials.unwrap_or(1),
        jobs: options.jobs.unwrap_or(4),
        policy: options.policy,
        out: options.out,
        pick: options.pick,
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let Some((verb, rest)) = arguments.split_first().filter(|(verb, _)| matches!(verb.as_str(), "promote" | "run" | "sweep")) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let options = match parse(rest) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{USAGE}\nnorth policy-replay: error: {message}");
            return 2;
        }
    };
    match verb.as_str() {
        "promote" => finish(promoted(&options)),
        "run" if options.arms.is_empty() || options.out.is_none() => {
            eprintln!("{USAGE}\nnorth policy-replay: error: --arm and --out are required for run");
            2
        }
        "run" => finish(arms(&Arms { arms: options.arms.clone(), common: common(options) })),
        _ => {
            let Some(variants) = options.variants.clone() else {
                eprintln!("{USAGE}\nnorth policy-replay: error: sweep needs --variants");
                return 2;
            };
            if options.pick.model.is_some() || options.pick.effort.is_some() || options.pick.advisor.is_some() {
                eprintln!("{USAGE}\nnorth policy-replay: error: --model, --effort and --advisor are per-run arms; sweep keeps each scenario's model");
                return 2;
            }
            let (only, provisional) = (options.only.clone(), options.provisional);
            let candidate = options.candidate.clone().unwrap_or_else(default_candidate);
            finish(sweep(&Sweep { variants, only, candidate, provisional, common: common(options) }))
        }
    }
}
