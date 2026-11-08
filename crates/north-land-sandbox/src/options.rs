use std::path::PathBuf;

pub struct Options {
    pub enforce: Option<PathBuf>,
    pub tree: Option<PathBuf>,
    pub scratch: Option<PathBuf>,
    pub command: Vec<String>,
}

impl Options {
    pub fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut options = Self { enforce: None, tree: None, scratch: None, command: Vec::new() };
        let mut rest = arguments.iter();
        while let Some(argument) = rest.next() {
            let slot = match argument.as_str() {
                "--enforce" => &mut options.enforce,
                "--tree" => &mut options.tree,
                "--scratch" => &mut options.scratch,
                "--" => {
                    options.command = rest.cloned().collect();
                    break;
                }
                other => return Err(format!("unknown argument {other}")),
            };
            *slot = Some(rest.next().ok_or_else(|| format!("{argument} needs a value"))?.into());
        }
        if options.command.is_empty() {
            return Err("no command after --".into());
        }
        Ok(options)
    }
}
