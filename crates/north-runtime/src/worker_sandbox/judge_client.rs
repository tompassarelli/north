//! The client side of the private-text judge for a confined caller (north#1553, judge_request.rs): inside a worker sandbox's domain the judge's words go to the outside broker with the text as the request's standard input, and the broker relays the judge's answer.

use std::fs::File;
use std::os::fd::AsFd;

use north_base::platform::private_text::AT_ONCE_ENV;

use super::broker;
use super::judge_request::{AT_ONCE, VERB};

/// `north check disclosure judge ARGUMENTS` inside a worker sandbox's domain, sent to the outside broker when it reads one text and no kinds file; `None` outside a domain or for any other words.
pub fn command(arguments: &[String]) -> Option<u8> {
    std::env::var_os(super::MARK)?;
    let [verb, option, text] = arguments else { return None };
    if verb != "judge" || option != "--text" {
        return None;
    }
    let at_once = std::env::var_os(AT_ONCE_ENV).is_some_and(|value| !value.is_empty());
    let words: Vec<String> = std::iter::once(VERB).chain(at_once.then_some(AT_ONCE)).map(str::to_owned).collect();
    let sent = match text.as_str() {
        "-" => broker::request_check(&words),
        file => File::open(file).map_err(|error| format!("cannot read {file}: {error}")).and_then(|input| broker::request_with_input(&words, input.as_fd())),
    };
    Some(sent.unwrap_or_else(|why| {
        eprintln!("disclosure judge: {why}");
        2
    }))
}
