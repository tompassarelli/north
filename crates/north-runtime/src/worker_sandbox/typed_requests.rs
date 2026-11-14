//! The requests the outside broker answers from the caller's words alone, starting no `north` of the client's: a reviewer's hold or accept (hold_request.rs), an evaluation unit (eval_unit_request.rs) and a host operation (host_op_request.rs).

use std::os::fd::OwnedFd;

use super::lane_grant::Host;
use super::peer::Peer;
use super::{eval_unit_request, hold_request, host_op_request};

const VERBS: [&str; 4] = [crate::runtime::review_hold::VERB, crate::runtime::review_accept::VERB, eval_unit_request::VERB, host_op_request::VERB];

/// Whether `arguments` name one of these requests.
pub(super) fn named(arguments: &[String]) -> bool {
    arguments.first().is_some_and(|verb| VERBS.contains(&verb.as_str()))
}

/// Answers the request `arguments` names from the caller `peer` on `host`, telling the client on `streams`.
pub(super) fn answer(arguments: &[String], streams: Vec<OwnedFd>, peer: &Peer, host: &Host) -> Result<u8, String> {
    if let Some(asked) = crate::runtime::review_hold::Ask::asked(arguments) {
        return hold_request::answer(asked, streams, peer, host);
    }
    if let Some(asked) = crate::runtime::review_accept::Accept::asked(arguments) {
        return hold_request::answer_accept(asked, streams, peer, host);
    }
    if let Some(asked) = eval_unit_request::Ask::asked(arguments) {
        return eval_unit_request::answer(asked, streams, peer, host);
    }
    host_op_request::Ask::asked(arguments).map_or_else(|| Err(format!("{} is not a request the broker answers from its words", arguments.join(" "))), |asked| host_op_request::answer(asked, streams, peer, host))
}
