use rusqlite::Connection;

use super::blake::text_hash;
use super::query::{plan_query, query_words};

struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound
    }
}

#[test]
fn text_hash_matches_blake2b_with_an_eight_byte_digest() {
    let cases: [(&str, i64); 6] = [
        ("", -1970711489451281740),
        ("abc", -2829645022057097895),
        (&"x".repeat(127), -8182863848303472644),
        (&"x".repeat(128), -8062319141637250046),
        (&"x".repeat(129), 1791892490181060003),
        (&"hello world".repeat(40), -28967673397644377),
    ];
    for (text, want) in cases {
        assert_eq!(text_hash(text), want, "{} bytes", text.len());
    }
}

#[test]
fn generated_query_expressions_parse_through_fts5() {
    let words = ["deck", "pink", "c289-ko-bottom", "tools/x.ts", "AND", "OR", "NOT", "(", ")", "\"", "*", "orch*", "naïve", "7,200/7,200", "--render", "a.b", ":", "^", "-", "'"];
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE VIRTUAL TABLE t USING fts5(text, tokenize='porter unicode61 remove_diacritics 2'); INSERT INTO t VALUES('deck pink c289 ko bottom tools x ts naive')").unwrap();
    let mut rng = Lcg(20261010);
    for _ in 0..3000 {
        let text = (0..rng.next(7)).map(|_| words[rng.next(words.len())]).collect::<Vec<_>>().join(" ");
        let Some(plan) = plan_query(&text, rng.next(5) == 0, false) else {
            assert!(query_words(&text).is_empty(), "{text}");
            continue;
        };
        let parses = |expr: &str| db.prepare("SELECT rowid FROM t WHERE t MATCH ?").unwrap().query_map([expr], |row| row.get::<_, i64>(0)).and_then(|rows| rows.collect::<Result<Vec<_>, _>>()).is_ok();
        assert!(plan.matches.iter().any(|expr| parses(expr)), "no expression parses for {text:?}: {:?}", plan.matches);
        if let Some(partial) = &plan.partial {
            assert!(parses(partial), "{partial}");
        }
    }
}

