#![cfg(test)]

use crate::hook_input::read_json;
use sha2::{Digest, Sha256};

struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn string(&mut self) -> String {
        use std::fmt::Write;
        let mut output = String::from("\"");
        for _ in 0..self.next() % 12 {
            let point = self.next() as u32 % 0x110000;
            match self.next() % 5 {
                0 => output.push_str(
                    ["\\\"", "\\\\", "\\/", "\\b", "\\f", "\\n", "\\r", "\\t"]
                        [self.next() as usize % 8],
                ),
                1 => {
                    write!(output, "\\u{:04x}", point & 0xffff).unwrap();
                }
                2 => output.push_str(
                    [
                        "\\ud800",
                        "\\udfff",
                        "\\ud83d\\ude00",
                        "\\ud800\\ud800",
                        "\\u0000",
                        "\\u007f",
                    ][self.next() as usize % 6],
                ),
                _ => {
                    if let Some(character) =
                        char::from_u32(point).filter(|c| *c >= ' ' && !matches!(c, '"' | '\\'))
                    {
                        output.push(character);
                    }
                }
            }
        }
        output.push('"');
        output
    }

    fn document(&mut self, depth: usize) -> String {
        match self.next() % if depth == 0 { 6 } else { 9 } {
            0 => ["null", "true", "false", "NaN", "Infinity", "-Infinity"]
                [self.next() as usize % 6]
                .into(),
            1 => format!(
                "{}{}",
                if self.next().is_multiple_of(2) {
                    "-"
                } else {
                    ""
                },
                self.next()
            ),
            2 => format!(
                "{}{}{}{}",
                self.next(),
                self.next(),
                self.next(),
                self.next()
            ),
            3 => format!(
                "{}{}.{}e{}{}",
                if self.next().is_multiple_of(2) {
                    "-"
                } else {
                    ""
                },
                self.next(),
                self.next(),
                if self.next().is_multiple_of(2) {
                    "-"
                } else {
                    "+"
                },
                self.next() % 600
            ),
            4 => format!("{:?}", f64::from_bits(self.next())),
            5 => self.string(),
            6 => {
                let count = self.next() % 5;
                let values: Vec<_> = (0..count).map(|_| self.document(depth - 1)).collect();
                format!("[ {} ]", values.join(" ,\n\t"))
            }
            _ => {
                let key = self.string();
                let first = self.document(depth - 1);
                let second = self.document(depth - 1);
                format!("{{ {key} : {first}, \"a\": 0, {key}: {second}, \"\\u0061\": 1 }}")
            }
        }
    }
}

fn documents() -> Vec<Vec<u8>> {
    let mut documents: Vec<Vec<u8>> = [
        "\"\\ud800\"",
        "\"\\udc00\"",
        "\"\\ud83d\\ude00\"",
        "\"\\ud800\\u0041\"",
        "\"\\ud800\\ud800\"",
        "{\"b\":1,\"a\":2,\"b\":3,\"\\u0061\":4}",
        "18446744073709551617",
        "-0",
        "-0.0",
        "1e23",
        "1e-4",
        "1e-5",
        "1e15",
        "1e16",
        "1e309",
        "-1e309",
        "1e-9999",
        "-1e-9999",
        "2.2250738585072012e-308",
        "5e-324",
        "714906239970683.3",
        "NaN",
        "Infinity",
        "-Infinity",
        "",
        " ",
        "[1,]",
        "{\"a\":1,}",
        "\"\\x00\"",
        "\"\\u000g\"",
        "\"\\ud800\\u000g\"",
        "\"\n\"",
        "\"\\v\"",
        "01",
        "+1",
        ".1",
        "1.",
        "1e",
        "1e+",
        "nan",
        "inf",
        "-NaN",
        "null null",
        "truex",
        "\u{feff}{}",
        "[",
        "{",
        "\"",
        "null\0",
        "{\"\\ud800\\udc00\":1,\"𐀀\":2}",
        "[\"\\b\\f\\n\\r\\t\\/\\\\\\\"\",\"\u{7f}\"]",
    ]
    .into_iter()
    .map(|text| text.as_bytes().to_vec())
    .collect();
    for length in [4300, 4301, 5000] {
        documents.push("9".repeat(length).into_bytes());
        documents.push(format!("-{}", "9".repeat(length)).into_bytes());
        documents.push(format!("{}.0", "9".repeat(length)).into_bytes());
    }
    for depth in [128, 998, 1000, 1500, 9994, 9995, 9996, 10000] {
        documents.push(format!("{}0{}", "[".repeat(depth), "]".repeat(depth)).into_bytes());
        documents
            .push(format!("{}0{}", "{\"a\":".repeat(depth), "}".repeat(depth)).into_bytes());
    }
    for text in ["{\"x\":\"é😀\"}", "true", "{}", "\"\\ud800\""] {
        let units: Vec<_> = text.encode_utf16().collect();
        for little in [false, true] {
            let utf16: Vec<u8> = units
                .iter()
                .flat_map(|unit| {
                    if little {
                        unit.to_le_bytes()
                    } else {
                        unit.to_be_bytes()
                    }
                })
                .collect();
            let utf32: Vec<u8> = text
                .chars()
                .flat_map(|c| {
                    if little {
                        u32::from(c).to_le_bytes()
                    } else {
                        u32::from(c).to_be_bytes()
                    }
                })
                .collect();
            documents.push(utf16.clone());
            documents.push(utf32.clone());
            documents.push(
                [
                    if little {
                        vec![0xff, 0xfe]
                    } else {
                        vec![0xfe, 0xff]
                    },
                    utf16,
                ]
                .concat(),
            );
            documents.push(
                [
                    if little {
                        vec![0xff, 0xfe, 0, 0]
                    } else {
                        vec![0, 0, 0xfe, 0xff]
                    },
                    utf32,
                ]
                .concat(),
            );
        }
        documents.push([vec![0xef, 0xbb, 0xbf], text.as_bytes().to_vec()].concat());
    }
    documents.extend([
        vec![b'"', 0xed, 0xa0, 0x80, b'"'],
        vec![b'"', 0xed, 0xa0, 0x80, 0xed, 0xb0, 0x80, b'"'],
        vec![b'"', 0xc0, 0x80, b'"'],
        vec![0xff],
        vec![0, 0, 0x11, 0, 0, 0, 0, 0],
    ]);
    let mut generator = Generator(0xd138_058c_a79b_4661);
    while documents.len() < 256 {
        let mut text = generator.document(5);
        match generator.next() % 8 {
            0 => text.push(','),
            1 => {
                text.pop();
            }
            2 => {
                text.insert(0, '\u{b}');
            }
            3 => text = format!(" \t\r\n{text}\r\n"),
            _ => {}
        }
        documents.push(text.into_bytes());
    }
    documents
}

fn records() -> Vec<(String, Vec<u8>)> {
    let mut records = Vec::new();
    for (index, line) in include_str!("tripwire_cases.jsonl").lines().enumerate() {
        records.push((format!("tripwire_cases.jsonl:{}", index + 1), line.as_bytes().to_vec()));
        let case: serde_json::Value = serde_json::from_str(line).unwrap();
        records.push((format!("tripwire_cases.jsonl:{} input", index + 1), case["input"].as_str().unwrap().as_bytes().to_vec()));
    }
    for (name, text) in [
        ("recommend_acts/acted.jsonl", include_str!("fixtures/recommend_acts/acted.jsonl")),
        ("recommend_acts/ask_once.jsonl", include_str!("fixtures/recommend_acts/ask_once.jsonl")),
        ("recommend_acts/edited.jsonl", include_str!("fixtures/recommend_acts/edited.jsonl")),
        ("recommend_acts/launched_only.jsonl", include_str!("fixtures/recommend_acts/launched_only.jsonl")),
        ("recommend_acts/other_issue.jsonl", include_str!("fixtures/recommend_acts/other_issue.jsonl")),
        ("recommend_acts/unacted.jsonl", include_str!("fixtures/recommend_acts/unacted.jsonl")),
    ] {
        records.extend(text.lines().enumerate().map(|(index, line)| (format!("{name}:{}", index + 1), line.as_bytes().to_vec())));
    }
    for (name, text) in [
        ("heavy_command.json", include_str!("fixtures/heavy_command.json")),
        ("proxy_load_trace_2026_11_11.json", include_str!("fixtures/proxy_load_trace_2026_11_11.json")),
        ("proxy_traced_launch_2026_11_11.json", include_str!("fixtures/proxy_traced_launch_2026_11_11.json")),
    ] {
        records.push((name.to_owned(), text.as_bytes().to_vec()));
    }
    records
}

fn reading(bytes: &[u8]) -> Option<String> {
    read_json(bytes).map(|value| format!("{:x}", Sha256::digest(serde_json::to_vec(&value).unwrap())))
}

#[test]
fn serde_json_reads_each_record_as_the_python_reader_did() {
    let python: Vec<_> = include_str!("fixtures/json_readings/cpython-3.13.15-dumps.jsonl").lines().map(|line| serde_json::from_str::<Option<String>>(line).unwrap()).collect();
    let old: Vec<_> = include_str!("fixtures/json_readings/records.tsv").lines().map(|line| line.split_once('\t').unwrap()).collect();
    let (documents, records) = (documents(), records());
    assert_eq!((documents.len(), records.len()), (python.len(), old.len()));
    let mut differences = Vec::new();
    for (index, (document, python)) in documents.iter().zip(python).enumerate() {
        let unreadable = !std::str::from_utf8(document).is_ok_and(|text| !text.starts_with('\u{feff}') && !text.contains('\0') && !text.contains("NaN") && !text.contains("Infinity"));
        if reading(document) != python.and_then(|text| reading(text.as_bytes())) && !unreadable && document != b"-0" {
            differences.push(format!("case {index}: {:?}", String::from_utf8_lossy(document)));
        }
    }
    for ((name, record), (named, old)) in records.into_iter().zip(old) {
        assert_eq!(name, named);
        if reading(&record).unwrap_or_default() != old {
            differences.push(name);
        }
    }
    let log = tempfile::tempdir().unwrap();
    let log = log.path().display().to_string();
    let env = |key: &str| match key {
        "AGENT_NO_AUTHORING_HOOKS" => "0".to_owned(),
        "TRIPWIRE_LOG_DIR" => log.clone(),
        _ => String::new(),
    };
    for hook in [
        br#"{"hook_event_name":"PreToolUse","tool_name":"Bash","cwd":"/tmp","tool_input":{"command":"rm -rf /","description":"\ud800"}}"#.as_slice(),
        br#"{"hook_event_name":"PreToolUse","tool_name":"functions.exec_command","cwd":"/tmp","tool_input":"{\"cmd\":\"rm -rf /\",\"opaque\":\"\\udc00\"}"}"#.as_slice(),
    ] {
        let input = crate::hook_input::HookInput::parse(hook).expect("a hook input with a lone surrogate escape still parses");
        assert!(matches!(crate::guard::tripwire::decide(&input, &env, &|_, _, _, _| None), crate::hook_input::Verdict::Deny(_)), "{}", String::from_utf8_lossy(hook));
    }
    assert_eq!(differences, Vec::<String>::new());
}
