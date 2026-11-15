use clap::error::{ContextKind, ErrorKind};
use clap::Parser;

pub(crate) fn parse<T: Parser>(arguments: &[String]) -> Result<T, String> {
    let value_flags: Vec<String> = T::command()
        .get_arguments()
        .filter(|argument| argument.get_action().takes_values())
        .filter_map(|argument| argument.get_long().map(|long| format!("--{long}")))
        .collect();
    reject_inline_values(arguments, &value_flags)?;
    T::try_parse_from(arguments.iter().map(String::as_str)).map_err(|error| usage_message(&error))
}

fn reject_inline_values(arguments: &[String], value_flags: &[String]) -> Result<(), String> {
    let mut value_expected = false;
    for argument in arguments {
        if std::mem::take(&mut value_expected) {
            continue;
        }
        if argument == "--" || (argument.starts_with("--") && argument.contains('=')) {
            return Err(format!("unknown argument {argument}"));
        }
        value_expected = value_flags.contains(argument);
    }
    Ok(())
}

fn usage_message(error: &clap::Error) -> String {
    let invalid = error.get(ContextKind::InvalidArg).map(ToString::to_string).unwrap_or_default();
    let flag = invalid.split_whitespace().next().unwrap_or_default();
    match error.kind() {
        ErrorKind::UnknownArgument => format!("unknown argument {invalid}"),
        ErrorKind::InvalidValue => format!("{flag} needs a value"),
        ErrorKind::MissingRequiredArgument => format!("{flag} is required"),
        _ => error.to_string(),
    }
}
