//! The client side of the planning call for a confined caller (north#1599, planner_request.rs): inside a worker sandbox's domain the call's model, effort, system prompt and input go to the outside broker, which runs it on a subscription seat and relays the answer back as JSON.

use std::io::{Read, Write};
use std::os::fd::AsFd;

use super::broker;
use super::planner_request::VERB;

/// The answer JSON of one planning call, `None` outside a worker sandbox's domain, where the caller asks its own subscription seat instead.
pub fn ask(model: &str, effort: &str, system: &str, seconds: u64, input: &str) -> Option<Result<String, String>> {
    std::env::var_os(super::MARK)?;
    let words: Vec<String> = [VERB, model, effort, system].into_iter().map(str::to_owned).chain([seconds.to_string()]).collect();
    Some(exchange(&words, input))
}

fn exchange(words: &[String], input: &str) -> Result<String, String> {
    let (input_read, mut input_write) = std::io::pipe().map_err(|error| error.to_string())?;
    let (mut output_read, output_write) = std::io::pipe().map_err(|error| error.to_string())?;
    std::thread::scope(|threads| {
        threads.spawn(move || input_write.write_all(input.as_bytes()));
        let answer = threads.spawn(move || {
            let mut text = String::new();
            output_read.read_to_string(&mut text).map(|_| text)
        });
        let code = broker::request_with_output(words, input_read.as_fd(), output_write.as_fd());
        drop(input_read);
        drop(output_write);
        let text = answer.join().map_err(|_| "the planning call's answer could not be read".to_owned())?.map_err(|error| error.to_string())?;
        match code? {
            0 => Ok(text),
            code => Err(format!("the planning call through the outside broker failed (exit {code})")),
        }
    })
}
