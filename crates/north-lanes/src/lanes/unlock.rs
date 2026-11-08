use std::path::PathBuf;

const USAGE: &str = "usage: north lane unlock [CHECKOUT...]

Removes each git lock file in CHECKOUT's git directory and common directory (default: the current
directory) that no process holds open and that is older than 60 s. It is the one removal an agent
may make in a primary checkout's git directory; a held or younger lock is kept and named.";

pub fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return 0;
    }
    if arguments.iter().any(|argument| argument.starts_with('-')) {
        eprintln!("{USAGE}");
        return 2;
    }
    let checkouts: Vec<PathBuf> = match arguments {
        [] => vec![PathBuf::from(".")],
        named => named.iter().map(PathBuf::from).collect(),
    };
    let mut code = 0;
    for checkout in checkouts {
        match north_git_locks::clear(&checkout) {
            Ok(sweep) if sweep.removed.is_empty() && sweep.kept.is_empty() => println!("lane unlock: {} has no git lock files", checkout.display()),
            Ok(sweep) => print!("{sweep}"),
            Err(error) => {
                eprintln!("lane unlock: {error}");
                code = 1;
            }
        }
    }
    code
}
