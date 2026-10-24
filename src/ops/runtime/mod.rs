pub(crate) mod layout;
mod update;

pub(crate) fn run(arguments: &[String]) -> u8 {
    if arguments != ["update"] {
        eprintln!("usage: north runtime update");
        return 64;
    }
    match update::update(&layout::Layout::from_env()) {
        Ok(()) => 0,
        Err(failure) => {
            eprintln!("north runtime update: {}", failure.message);
            failure.code
        }
    }
}
