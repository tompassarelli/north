#[derive(Clone, Copy, Debug)]
pub enum Slot {
    Var(usize),
    Const(i64),
}
