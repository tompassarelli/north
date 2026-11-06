use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub type Row = Rc<[i64]>;
type Index = (Vec<usize>, HashMap<Vec<i64>, Vec<usize>>);

pub struct DerivedRelation {
    rows: Vec<Row>,
    set: HashSet<Row>,
    indexes: Vec<Index>,
}

impl DerivedRelation {
    pub fn new(masks: &[Vec<usize>]) -> Self {
        Self { rows: Vec::new(), set: HashSet::new(), indexes: masks.iter().map(|mask| (mask.clone(), HashMap::new())).collect() }
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn insert(&mut self, row: Row) -> bool {
        if !self.set.insert(Rc::clone(&row)) {
            return false;
        }
        let position = self.rows.len();
        for (mask, index) in &mut self.indexes {
            index.entry(mask.iter().map(|column| row[*column]).collect()).or_default().push(position);
        }
        self.rows.push(row);
        true
    }

    pub fn lookup(&self, key: &[Option<i64>]) -> Vec<Row> {
        let mask: Vec<usize> = key.iter().enumerate().filter(|(_, value)| value.is_some()).map(|(column, _)| column).collect();
        let values: Vec<i64> = key.iter().flatten().copied().collect();
        if mask.is_empty() {
            return self.rows.clone();
        }
        if mask.len() == key.len() {
            return self.set.get(values.as_slice()).cloned().into_iter().collect();
        }
        match self.indexes.iter().find(|(known, _)| *known == mask) {
            Some((_, index)) => index.get(&values).into_iter().flatten().map(|position| Rc::clone(&self.rows[*position])).collect(),
            None => self.rows.iter().filter(|row| mask.iter().zip(&values).all(|(column, value)| row[*column] == *value)).cloned().collect(),
        }
    }
}
