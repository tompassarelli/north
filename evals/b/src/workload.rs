use std::fs;
use std::path::Path;

use serde_json::{Value, json};

pub const SHARED_EVERY: usize = 3;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn word(&mut self) -> String {
        const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzAEIOUBCDXYZ0123456789";
        let length = 2 + self.below(6) as usize;
        (0..length).map(|_| LETTERS[self.below(LETTERS.len() as u64) as usize] as char).collect()
    }

    fn input(&mut self) -> String {
        let words = 1 + self.below(4) as usize;
        (0..words).map(|_| self.word()).collect::<Vec<_>>().join(" ")
    }
}

#[derive(Debug)]
enum Kind {
    Shift(u8),
    ReverseWords,
    RunLength,
    DropVowels,
    TitleCase,
    DigitSum(u32),
    Repeat(usize),
    SortChars,
    Squeeze,
    Mask(usize),
    CountChar(char),
    SwapCase,
}

impl Kind {
    fn pick(rng: &mut Rng, index: usize) -> Self {
        match index % 12 {
            0 => Self::Shift(1 + rng.below(25) as u8),
            1 => Self::ReverseWords,
            2 => Self::RunLength,
            3 => Self::DropVowels,
            4 => Self::TitleCase,
            5 => Self::DigitSum(2 + rng.below(8) as u32),
            6 => Self::Repeat(2 + rng.below(3) as usize),
            7 => Self::SortChars,
            8 => Self::Squeeze,
            9 => Self::Mask(2 + rng.below(4) as usize),
            10 => Self::CountChar(['a', 'e', 'o', 'x', '1'][rng.below(5) as usize]),
            _ => Self::SwapCase,
        }
    }

    fn spec(&self) -> String {
        match self {
            Self::Shift(k) => format!("shifts every ASCII letter forward by {k} places in the alphabet, wrapping from z to a and from Z to A and keeping its case; every other character stays as it is"),
            Self::ReverseWords => "reverses the characters of each space-separated word and keeps the words in their order and the single spaces between them".to_owned(),
            Self::RunLength => "run-length encodes the input: each run of one repeated character becomes that character followed by the run's length in decimal, so \"aaab\" becomes \"a3b1\" and \"\" stays \"\"".to_owned(),
            Self::DropVowels => "removes every vowel (a, e, i, o, u in either case) and keeps every other character in order".to_owned(),
            Self::TitleCase => "upper-cases the first character of each space-separated word and lower-cases the rest of the word, keeping the single spaces".to_owned(),
            Self::DigitSum(m) => format!("returns, as a decimal string, the sum of the input's decimal digits multiplied by {m}, so an input with no digits gives \"0\""),
            Self::Repeat(k) => format!("repeats every character {k} times in place, so \"ab\" becomes the character a {k} times followed by b {k} times"),
            Self::SortChars => "returns the input's characters sorted in ascending UTF-16 code unit order, spaces included".to_owned(),
            Self::Squeeze => "collapses every run of one repeated character to a single copy of it, so \"aabbba\" becomes \"aba\"".to_owned(),
            Self::Mask(k) => format!("replaces with \"*\" each character whose position, counting from 1 with spaces included, is a multiple of {k}, and keeps the others"),
            Self::CountChar(c) => format!("returns, as a decimal string, how many times the character \"{c}\" occurs in the input, case-sensitively"),
            Self::SwapCase => "swaps the case of every ASCII letter and keeps every other character".to_owned(),
        }
    }

    fn apply(&self, input: &str) -> String {
        self.per_char(input).unwrap_or_else(|| self.whole(input))
    }

    fn per_char(&self, input: &str) -> Option<String> {
        let vowel = |c: char| "aeiouAEIOU".contains(c);
        Some(match self {
            Self::Shift(k) => input.chars().map(|c| shift(c, *k)).collect(),
            Self::DropVowels => input.chars().filter(|c| !vowel(*c)).collect(),
            Self::Repeat(k) => input.chars().flat_map(|c| std::iter::repeat_n(c, *k)).collect(),
            Self::Mask(k) => input.chars().enumerate().map(|(i, c)| if (i + 1) % k == 0 { '*' } else { c }).collect(),
            Self::CountChar(c) => input.chars().filter(|x| x == c).count().to_string(),
            Self::SwapCase => input.chars().map(|c| if c.is_ascii_lowercase() { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() }).collect(),
            _ => return None,
        })
    }

    fn whole(&self, input: &str) -> String {
        match self {
            Self::ReverseWords => input.split(' ').map(|w| w.chars().rev().collect::<String>()).collect::<Vec<_>>().join(" "),
            Self::RunLength => runs(input).into_iter().map(|(c, n)| format!("{c}{n}")).collect(),
            Self::TitleCase => input.split(' ').map(title).collect::<Vec<_>>().join(" "),
            Self::DigitSum(m) => (input.chars().filter_map(|c| c.to_digit(10)).sum::<u32>() * m).to_string(),
            Self::SortChars => {
                let mut chars: Vec<char> = input.chars().collect();
                chars.sort_unstable();
                chars.into_iter().collect()
            }
            _ => runs(input).into_iter().map(|(c, _)| c).collect(),
        }
    }
}

fn shift(c: char, k: u8) -> char {
    let base = match c {
        'a'..='z' => b'a',
        'A'..='Z' => b'A',
        _ => return c,
    };
    ((c as u8 - base + k) % 26 + base) as char
}

fn runs(input: &str) -> Vec<(char, usize)> {
    let mut out: Vec<(char, usize)> = Vec::new();
    for c in input.chars() {
        match out.last_mut() {
            Some((last, n)) if *last == c => *n += 1,
            _ => out.push((c, 1)),
        }
    }
    out
}

fn title(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|first| first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()).unwrap_or_default()
}

pub fn id(index: usize) -> String {
    format!("t{:02}", index + 1)
}

pub fn shared(index: usize) -> bool {
    index % SHARED_EVERY == SHARED_EVERY - 1
}

fn test_file(id: &str, kind: &Kind, rng: &mut Rng, shared: bool, skip: bool) -> String {
    let mut body = format!("import {{ expect, test }} from \"bun:test\";\nimport {{ {id} }} from \"../src/{id}\";\n");
    if shared {
        body.push_str("import { registry } from \"../src/registry\";\n");
    }
    body.push_str(&format!("\ntest{}(\"{id}\", () => {{\n", if skip { ".skip" } else { "" }));
    let inputs: Vec<String> = std::iter::once(String::new()).chain((0..4).map(|_| rng.input())).collect();
    for input in &inputs {
        body.push_str(&format!("  expect({id}({})).toBe({});\n", json!(input), json!(kind.apply(input))));
    }
    if shared {
        let input = &inputs[1];
        body.push_str(&format!("  expect(registry.{id}({})).toBe({});\n", json!(input), json!(kind.apply(input))));
    }
    body.push_str("});\n");
    body
}

pub struct Workload {
    pub tasks: Vec<Value>,
}

pub fn generate(seed: u64, count: usize, lander_host: &str, root: &Path, expect: &Path) -> std::io::Result<Workload> {
    let mut rng = Rng(seed);
    fs::create_dir_all(root.join("src"))?;
    fs::create_dir_all(root.join("test"))?;
    fs::create_dir_all(expect)?;
    fs::write(root.join("package.json"), "{\n  \"name\": \"eval-b-workload\",\n  \"private\": true,\n  \"type\": \"module\"\n}\n")?;
    fs::write(root.join("AGENTS.md"), "# Eval B workload\n\nA synthetic repository for north's load ladder. `bun test` from the repository root is the whole check and runs in about a second.\nEach task names the files it changes; change only those.\n")?;
    fs::write(root.join(".safe-push"), format!("[landing]\n\tqueue = lander\n\tlander-host = {lander_host}\n[land]\n\tcheck = ** :: bun test\n"))?;
    fs::write(root.join("src/echo.ts"), "export function echo(input: string): string {\n  return input;\n}\n")?;
    fs::write(root.join("src/registry.ts"), "import { echo } from \"./echo\";\n\nexport const registry: Record<string, (input: string) => string> = {\n  echo,\n};\n")?;
    fs::write(root.join("test/echo.test.ts"), "import { expect, test } from \"bun:test\";\nimport { registry } from \"../src/registry\";\n\ntest(\"echo\", () => {\n  expect(registry.echo(\"a b\")).toBe(\"a b\");\n});\n")?;
    let mut tasks = Vec::new();
    for index in 0..count {
        let id = id(index);
        let slot = (index * 7 + rng.below(12) as usize) % 12;
        let kind = Kind::pick(&mut rng, slot);
        let shared = shared(index);
        let state = rng.0;
        fs::write(root.join(format!("src/{id}.ts")), format!("export function {id}(input: string): string {{\n  throw new Error(\"{id} is not implemented for \" + input);\n}}\n"))?;
        fs::write(root.join(format!("test/{id}.test.ts")), test_file(&id, &kind, &mut rng, shared, true))?;
        rng.0 = state;
        fs::write(expect.join(format!("{id}.test.ts")), test_file(&id, &kind, &mut rng, shared, false))?;
        let mut text = format!(
            "Implement `function {id}(input: string): string` in `src/{id}.ts`, which now only throws, so that it {}. Then turn its test on: in `test/{id}.test.ts` change `test.skip(` to `test(` and change nothing else in that file.",
            kind.spec()
        );
        let mut files = vec![format!("src/{id}.ts"), format!("test/{id}.test.ts")];
        if shared {
            text.push_str(&format!(" Also register it in `src/registry.ts`: import `{id}` from `./{id}` and add `{id}` to the `registry` object, keeping both the import lines and the object's entries sorted by name."));
            files.push("src/registry.ts".to_owned());
        }
        let check = format!("bun test test/{id}.test.ts && ! grep -q 'test.skip' test/{id}.test.ts");
        tasks.push(json!({"id": id, "index": index, "shared": shared, "kind": format!("{kind:?}"), "text": text, "check": check, "files": files}));
    }
    Ok(Workload { tasks })
}
