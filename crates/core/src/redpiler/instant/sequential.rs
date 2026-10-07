//! Decision dependencies used by compile-time electrical/sampling extraction.
use super::boolean::{BooleanArena, Expr, Variable};
use rustc_hash::FxHashSet;

pub(crate) fn dependencies(
    arena: &BooleanArena,
    roots: impl IntoIterator<Item = Expr>,
) -> Vec<Variable> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut visited = FxHashSet::default();
    let mut variables = FxHashSet::default();
    while let Some(root) = pending.pop() {
        if visited.insert(root) {
            if let Some(d) = arena.decision(root) {
                variables.insert(d.variable);
                pending.extend([d.low, d.high]);
            }
        }
    }
    variables.into_iter().collect()
}
