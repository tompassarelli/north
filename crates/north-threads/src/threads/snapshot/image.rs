//! The image the ledger host builds: a SQLite backup of the ledger in memory, with every native item and
//! every free page zeroed so a caller never receives what it may not read.
use std::io;
use std::ops::Range;
use std::path::Path;
use std::time::Duration;

use rusqlite::backup::{Backup, StepResult};
use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, OpenFlags};

use super::pages::{be16, be32, invalid, page_size};
use crate::threads::catalog::CATALOG;
use crate::threads::verb::names_native_ref;

pub(super) fn image(database: &Path, withhold_native: bool) -> io::Result<Vec<u8>> {
    let source = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(sql)?;
    source.busy_timeout(Duration::from_secs(10)).map_err(sql)?;
    let size: i64 = source.query_row("PRAGMA page_size", [], |row| row.get(0)).map_err(sql)?;
    let mut memory = Connection::open_in_memory().map_err(sql)?;
    memory.execute_batch(&format!("PRAGMA page_size={size}")).map_err(sql)?;
    {
        let backup = Backup::new(&source, &mut memory).map_err(sql)?;
        while backup.step(-1).map_err(sql)? != StepResult::Done {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    if withhold_native {
        remove_native(&memory).map_err(sql)?;
    }
    let btree: Vec<u32> = memory.prepare("SELECT pageno FROM dbstat WHERE pagetype IN ('internal', 'leaf')").map_err(sql)?
        .query_map([], |row| row.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    let mut bytes = memory.serialize("main").map_err(sql)?.to_vec();
    normalize(&mut bytes, &btree)?;
    Ok(bytes)
}

/// Deletes every native item and every row any of whose text names a native ref, and the index
/// samples ANALYZE keeps, which can quote one; secure delete zeroes what the rows held.
fn remove_native(db: &Connection) -> rusqlite::Result<()> {
    db.create_scalar_function("names_native", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| {
        Ok(context.get_raw(0).as_str().is_ok_and(names_native_ref))
    })?;
    db.execute_batch("PRAGMA secure_delete=ON")?;
    let namespaces = CATALOG.native.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>().join(", ");
    db.execute(&format!("DELETE FROM items WHERE ns IN ({namespaces})"), [])?;
    let tables: Vec<String> = db.prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")?
        .query_map([], |row| row.get(0))?.collect::<Result<_, _>>()?;
    for table in tables {
        let columns: Vec<String> = db.prepare("SELECT name FROM pragma_table_info(?1)")?.query_map([&table], |row| row.get(0))?.collect::<Result<_, _>>()?;
        let named = columns.iter().map(|column| format!("names_native({})", identifier(column))).collect::<Vec<_>>().join(" OR ");
        db.execute(&format!("DELETE FROM {} WHERE {named}", identifier(&table)), [])?;
    }
    let sampled: bool = db.query_row("SELECT count(*) > 0 FROM sqlite_schema WHERE name = 'sqlite_stat4'", [], |row| row.get(0))?;
    if sampled {
        db.execute("DELETE FROM sqlite_stat4", [])?;
    }
    Ok(())
}

fn identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn normalize(image: &mut [u8], btree: &[u32]) -> io::Result<()> {
    let size = page_size(image).filter(|size| image.len().is_multiple_of(*size)).ok_or_else(|| invalid("not a whole SQLite image"))?;
    let count = image.len() / size;
    let usable = size - usize::from(image[20]);
    let page = |number: u32| -> io::Result<Range<usize>> {
        let number = number as usize;
        if number == 0 || number > count {
            return Err(invalid("a page reference lies outside the image"));
        }
        Ok((number - 1) * size..number * size)
    };
    image[18] = 1;
    image[19] = 1;
    let mut trunk = be32(&image[32..]);
    let mut hops = 0;
    while trunk != 0 {
        hops += 1;
        let range = page(trunk)?;
        let next = be32(&image[range.start..]);
        let leaves = be32(&image[range.start + 4..]) as usize;
        if hops > count || 8 + 4 * leaves > usable {
            return Err(invalid("the freelist is malformed"));
        }
        for index in 0..leaves {
            let leaf = page(be32(&image[range.start + 8 + 4 * index..]))?;
            image[leaf].fill(0);
        }
        image[range.start + 8 + 4 * leaves..range.end].fill(0);
        trunk = next;
    }
    for &number in btree {
        let range = page(number)?;
        clear_free_space(&mut image[range][..usable], if number == 1 { 100 } else { 0 })?;
    }
    Ok(())
}

fn clear_free_space(page: &mut [u8], header: usize) -> io::Result<()> {
    let size = match page[header] {
        2 | 5 => 12,
        10 | 13 => 8,
        _ => return Err(corrupt()),
    };
    let cells = usize::from(be16(&page[header + 3..]));
    let content = match be16(&page[header + 5..]) {
        0 => 65536,
        start => usize::from(start),
    };
    let pointers = header + size + 2 * cells;
    if pointers > content || content > page.len() {
        return Err(corrupt());
    }
    page[pointers..content].fill(0);
    let block = usize::from(be16(&page[header + 1..]));
    clear_blocks(page, block)
}

/// Zeroes the bytes of every block in the page's free-block chain, which starts at `block`.
fn clear_blocks(page: &mut [u8], mut block: usize) -> io::Result<()> {
    let mut hops = 0;
    while block != 0 {
        hops += 1;
        if hops > page.len() / 4 || block + 4 > page.len() {
            return Err(corrupt());
        }
        let next = usize::from(be16(&page[block..]));
        let length = usize::from(be16(&page[block + 2..]));
        if length < 4 || block + length > page.len() {
            return Err(corrupt());
        }
        page[block + 4..block + length].fill(0);
        block = next;
    }
    Ok(())
}

fn corrupt() -> io::Error {
    invalid("a b-tree page header is malformed")
}

fn sql(error: rusqlite::Error) -> io::Error {
    io::Error::other(error.to_string())
}
