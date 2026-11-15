use crate::term::Term;
use crate::{parse_arg, term_from_json, term_to_json};

#[test]
fn terms_round_trip_through_their_text_and_json_forms() {
    let terms = [
        Term::string("a \"quoted\" (word)"),
        Term::int(-7),
        Term::float(1.5),
        Term::float(-0.0),
        Term::float(f64::NAN),
        Term::boolean(false),
        Term::keyword("kernel/op-ordinal"),
        Term::instant(-5, 7).unwrap(),
        Term::triple(Term::triple("a", Term::keyword("b"), 3), "c d", Term::float(2.0)),
    ];
    for term in terms {
        assert_eq!(parse_arg(&term.to_string()).unwrap(), term, "text form {term}");
        assert_eq!(term_from_json(&term_to_json(&term)).unwrap(), term, "json form {term}");
    }
    assert_eq!(parse_arg("north#669").unwrap(), Term::string("north#669"));
    assert!(parse_arg("(a b").is_err());
}
