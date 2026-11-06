use std::cmp::Ordering;

use super::ast::{Arith, Compare};
use crate::term::{Atom, Term, numeric_cmp};

pub fn compare(op: Compare, left: &Term, right: &Term) -> bool {
    match op {
        Compare::Eq => left == right,
        Compare::Ne => left != right,
        ordered => numeric_cmp(left, right).is_some_and(|order| match ordered {
            Compare::Lt => order == Ordering::Less,
            Compare::Le => order != Ordering::Greater,
            Compare::Gt => order == Ordering::Greater,
            Compare::Ge => order != Ordering::Less,
            Compare::Eq | Compare::Ne => false,
        }),
    }
}

pub fn arithmetic(op: Arith, left: &Term, right: &Term) -> Option<Term> {
    match (left, right) {
        (Term::Atom(Atom::Int(a)), Term::Atom(Atom::Int(b))) => integer(op, *a, *b).map(Term::int),
        _ => {
            let (a, b) = (number(left)?, number(right)?);
            let value = match op {
                Arith::Add => a + b,
                Arith::Sub => a - b,
                Arith::Mul => a * b,
                Arith::Div => a / b,
                Arith::Mod => a % b,
            };
            value.is_finite().then(|| Term::float(value))
        }
    }
}

fn integer(op: Arith, a: i64, b: i64) -> Option<i64> {
    match op {
        Arith::Add => a.checked_add(b),
        Arith::Sub => a.checked_sub(b),
        Arith::Mul => a.checked_mul(b),
        Arith::Div => a.checked_div(b),
        Arith::Mod => a.checked_rem(b),
    }
}

pub fn number(term: &Term) -> Option<f64> {
    match term {
        Term::Atom(Atom::Int(value)) => Some(*value as f64),
        Term::Atom(Atom::Float(value)) => Some(value.value()),
        _ => None,
    }
}
