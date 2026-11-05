use std::collections::HashSet;
use std::io::Cursor;

use rusqlite::Connection;

use super::blake::text_hash;
use super::lines::scan_lines;
use super::query::{Hit, plan_query, query_words};
use super::rank::{is_convo_call, rank_sessions, snippet};
use super::text::stem;

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
fn scan_lines_keeps_complete_line_offsets_and_bounds_long_lines_for_any_chunking() {
    let mut rng = Lcg(20261010);
    for _ in 0..400 {
        let alphabet = b"ab{}\" ";
        let lines: Vec<Vec<u8>> = (0..rng.next(13)).map(|_| (0..rng.next(71)).map(|_| alphabet[rng.next(alphabet.len())]).collect()).collect();
        let mut data = lines.join(&b'\n');
        if rng.next(2) == 0 {
            data.push(b'\n');
        }
        let mut got = Vec::new();
        let chunk = 1 + rng.next(64);
        scan_lines(&mut Cursor::new(data.clone()), 0, chunk, 40, |start, end, line| {
            got.push((start, end, line.map(<[u8]>::to_vec)));
            Ok(())
        })
        .unwrap();
        let mut want = Vec::new();
        let mut start = 0u64;
        let mut parts: Vec<&[u8]> = data.split(|byte| *byte == b'\n').collect();
        parts.pop();
        for part in parts {
            let end = start + part.len() as u64 + 1;
            want.push((start, end, (part.len() <= 40).then(|| part.to_vec())));
            start = end;
        }
        assert_eq!(got, want, "{data:?}");
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

#[test]
fn snippets_stay_bounded_and_retain_the_match() {
    let mut rng = Lcg(7);
    for _ in 0..500 {
        let pool = ["alpha", "beta", "gamma", "tests"];
        let long = "x".repeat(1 + rng.next(30));
        let words: Vec<String> = (0..1 + rng.next(120)).map(|_| if rng.next(5) == 0 { long.clone() } else { pool[rng.next(4)].to_owned() }).collect();
        let term = words[rng.next(words.len())].clone();
        let shown = snippet(&words.join(" "), std::slice::from_ref(&term), 80);
        assert!(shown.chars().count() <= 82, "{shown}");
        assert!(shown.to_lowercase().contains(&stem(&term)), "{term} {shown}");
    }
}

fn hit(id: i64, session: &str, role: &str, text: &str, bm25: f64) -> Hit {
    Hit {
        id, line: id, ts: None, role: role.to_owned(), tool: None, bm25, session: session.to_owned(), agent: None, path: String::new(), project: None, text: text.to_owned(), w: 0.0,
    }
}

#[test]
fn ranked_sessions_conserve_non_call_hits_and_put_the_caller_last() {
    let mut rng = Lcg(99);
    let kinds = [("tool", "Bash command=convo deck pink"), ("assistant", "deck"), ("user", "pink deck")];
    let mine: HashSet<String> = ["A".to_owned()].into();
    for _ in 0..300 {
        let hits: Vec<Hit> = (0..1 + rng.next(30))
            .map(|id| {
                let (role, text) = kinds[rng.next(3)];
                hit(id as i64, &"ABCD"[rng.next(4)..][..1], role, text, -(rng.next(1000) as f64) / 100.0)
            })
            .collect();
        let groups = rank_sessions(&hits, &["deck".to_owned()], &mine, false, false);
        let order: Vec<&str> = groups.iter().map(|group| group.session.as_str()).collect();
        let unique: HashSet<&&str> = order.iter().collect();
        assert_eq!(order.len(), unique.len());
        assert!(!order[..order.len().saturating_sub(1)].contains(&"A"), "{order:?}");
        assert_eq!(groups.iter().map(|group| group.hits).sum::<usize>(), hits.iter().filter(|hit| !is_convo_call(hit)).count());
        assert!(groups.iter().all(|group| !is_convo_call(&group.best)));
    }
}
