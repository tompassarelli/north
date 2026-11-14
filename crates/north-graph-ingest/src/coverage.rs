use north_coverage::map::{Function, Index, Map};
use north_graph::Telling;

use crate::facts::{Facts, text};
use crate::places::Places;

pub(crate) const WRITER: &str = "coverage";
const REPLACED: [&str; 7] = ["commit", "reaches", "in-file", "in-binary", "item-in", "defines", "refers-to"];

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
            let node = function_node(function);
            facts.tell(WRITER, text(&node), "in-file", text(&function.file));
            for reached in function.reached_by.iter().filter_map(|index| tests.get(usize::try_from(*index).ok()?)) {
                facts.tell(WRITER, text(reached), "reaches", text(&node));
            }
        }
        tell_index(&mut facts, &map.functions, &map.index);
    }
    Ok(facts.finish())
}

fn function_node(function: &Function) -> String {
    format!("fn:{}:{}-{}", function.file, function.start, function.end)
}

fn tell_index(facts: &mut Facts, functions: &[Function], index: &Index) {
    let symbol = |number: &u32| index.symbols.get(usize::try_from(*number).ok()?).map(|name| text(format!("sym:{name}")));
    for item in &index.items {
        let node = format!("item:{}:{}-{}", item.file, item.start, item.end);
        facts.tell(WRITER, text(&node), "item-in", text(&item.file));
        for defined in item.defines.iter().filter_map(symbol) {
            facts.tell(WRITER, text(&node), "defines", defined);
        }
        for used in item.uses.iter().filter_map(symbol) {
            facts.tell(WRITER, text(&node), "refers-to", used);
        }
    }
    for refers in &index.refers {
        let Some(function) = usize::try_from(refers.function).ok().and_then(|number| functions.get(number)) else { continue };
        let node = function_node(function);
        for used in refers.symbols.iter().filter_map(symbol) {
            facts.tell(WRITER, text(&node), "refers-to", used);
        }
    }
}
