use std::path::PathBuf;

pub struct Options {
    pub enforce: Option<PathBuf>,
    pub record: Option<PathBuf>,
    pub tree: Option<PathBuf>,
    pub scratch: Option<PathBuf>,
    pub trace: bool,
    pub command: Vec<String>,
}

impl Options {
    pub fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut options = Self { enforce: None, record: None, tree: None, scratch: None, trace: false, command: Vec::new() };
        let mut rest = arguments.iter();
        while let Some(argument) = rest.next() {
            match argument.as_str() {
                "--enforce" => options.enforce = Some(take(&mut rest, argument)?.into()),
                "--record" => options.record = Some(take(&mut rest, argument)?.into()),
                "--tree" => options.tree = Some(take(&mut rest, argument)?.into()),
                "--scratch" => options.scratch = Some(take(&mut rest, argument)?.into()),
                "--trace" => options.trace = true,
                "--" => {
                    options.command = rest.cloned().collect();
                    break;
                }
                other => return Err(format!("unknown argument {other}")),
            }
        }
        if options.command.is_empty() {
            return Err("no command after --".into());
        }
        Ok(options)
    }
}

fn take<'a>(rest: &mut std::slice::Iter<'a, String>, argument: &str) -> Result<&'a String, String> {
    rest.next().ok_or_else(|| format!("{argument} needs a value"))
}
