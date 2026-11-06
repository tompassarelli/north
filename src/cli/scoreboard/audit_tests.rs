#![cfg(test)]

use super::{Verdict, judge, parse_verdicts};
use crate::guard::recommend_acts::{Issue, Turn};

fn turn(id: &str, reply: &str, moved: bool) -> Turn {
    Turn { id: id.into(), reply: Some(reply.into()), moved: if moved { vec![Issue { repo: Some("north".into()), number: 641 }] } else { Vec::new() }, ..Turn::default() }
}

fn verdict(id: &str, recommends: bool, phrase: &str, names_goal: bool) -> Verdict {
    Verdict { id: id.into(), recommends, phrase: phrase.into(), names_goal }
}

#[test]
fn the_audit_counts_unfiled_recommendations_proposes_each_missed_phrasing_and_counts_ungoaled_ones() {
    let acted = turn("a", "I recommend the lander change, tracked on north#641; it serves the landing-p90 goal.", true);
    let unacted = turn("b", "I recommend moving the lander; it serves nothing named.", false);
    let missed = turn("c", "The cleaner path is to split it three ways. Which do you prefer?", false);
    let quiet = turn("d", "Landed as 8e0acfd.", false);
    let asked_once = turn("e", "I recommend minting the GitHub token.", false);
    let turns = [&acted, &unacted, &missed, &quiet, &asked_once];
    let verdicts = [verdict("a", true, "I recommend", true), verdict("b", true, "I recommend", false), verdict("c", true, "Which do you prefer", false), verdict("d", false, "", false)];
    let audit = judge(&turns, &verdicts);
    assert_eq!((audit.read, audit.recommendations, audit.unfiled, audit.ungoaled), (5, 3, 2, 2));
    assert_eq!(audit.missed, ["Which do you prefer"]);
    assert_eq!(audit.ask_once.get("token"), Some(&1));
    let answer = "Here you go:\n[{\"id\":\"a\",\"recommends\":true,\"phrase\":\"I suggest\",\"names_goal\":false}]\n";
    assert_eq!(parse_verdicts(answer).unwrap(), vec![verdict("a", true, "I suggest", false)], "the verdicts are read from the array the model wraps in prose");
    assert!(parse_verdicts("no array").is_err());
    assert!(audit.line().contains("2 became no issue (phrasings to add to TRIGGERS in recommend_acts.rs: \"Which do you prefer\")") && audit.line().contains("ask-once passes by word: token 1"));
}
