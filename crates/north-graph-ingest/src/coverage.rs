use north_coverage::map::Map;
use north_graph::Telling;

use crate::facts::{Facts, text};
use crate::places::Places;

const WRITER: &str = "coverage";
const REPLACED: [&str; 4] = ["commit", "reaches", "in-file", "in-binary"];

pub fn read(places: &Places) -> Result<Telling, String> {
    let mut maps: Vec<_> = std::fs::read_dir(&places.coverage)
        .map_err(|why| format!("{}: {why}", places.coverage.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .collect();
    maps.sort();
    let mut facts = Facts::new(&REPLACED);
    for path in maps {
        let map = Map::load(&path)?;
        let repository = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        facts.tell(WRITER, text(format!("coverage:{repository}")), "commit", text(&map.commit));
        let tests: Vec<String> = map.tests.iter().map(|test| format!("test:{}::{}", test.binary, test.name)).collect();
        for test in map.tests.iter().zip(&tests) {
            facts.tell(WRITER, text(test.1), "in-binary", text(&test.0.binary));
        }
        for function in &map.functions {
            let node = format!("fn:{}:{}-{}", function.file, function.start, function.end);
            facts.tell(WRITER, text(&node), "in-file", text(&function.file));
            for reached in function.reached_by.iter().filter_map(|index| tests.get(usize::try_from(*index).ok()?)) {
                facts.tell(WRITER, text(reached), "reaches", text(&node));
            }
        }
    }
    Ok(facts.finish())
}
