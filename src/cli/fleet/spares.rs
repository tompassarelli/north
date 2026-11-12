//! `north spare-fill NAME`: the fill each launch of a worker on a build host starts apart from itself, keeping spares there for the next launch like it.
use super::router::open_fleet;

pub fn fill(arguments: &[String]) -> u8 {
    let [name] = arguments else {
        eprintln!("Usage: north spare-fill NAME");
        return 2;
    };
    match open_fleet().and_then(|fleet| fleet.fill_spares(name)) {
        Ok(said) => {
            println!("{said}");
            0
        }
        Err(error) => {
            eprintln!("north spare-fill: {}", error.user_message());
            1
        }
    }
}
