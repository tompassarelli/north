use crate::term::Term;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Arg {
    Var(String),
    Const(Term),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Compare {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Clause {
    Relation { name: String, args: Vec<Arg>, negated: bool },
    Predicate { op: Compare, left: Arg, right: Arg },
    Function { op: Arith, left: Arg, right: Arg, bind: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rule {
    pub head: String,
    pub head_args: Vec<Arg>,
    pub body: Vec<Clause>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AggregateOp {
    Count,
    CountDistinct,
    Sum,
    Avg,
    Min,
    Max,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Aggregate {
    pub op: AggregateOp,
    pub column: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Having {
    pub op: Compare,
    pub aggregate: usize,
    pub value: Term,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AggregateFind {
    pub relation: String,
    pub group: Vec<usize>,
    pub aggregates: Vec<Aggregate>,
    pub having: Vec<Having>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Find {
    Relation(String),
    Aggregate(AggregateFind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Order {
    pub column: usize,
    pub descending: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Query {
    pub find: Find,
    pub strata: Vec<Vec<Rule>>,
    pub order_by: Vec<Order>,
    pub limit: Option<usize>,
    pub timeout_ms: Option<u64>,
}
