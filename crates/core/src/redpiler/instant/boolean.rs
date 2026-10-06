//! Canonical Boolean functions used by geometric wave extraction. Node zero
//! and one are terminals; every other node is an ordered decision on a port
//! threshold or a provisional actuator response.
use mchprs_blocks::BlockPos;
use rustc_hash::FxHashMap;
use std::cmp::Ordering;

pub(crate) type Expr = u32;
pub(crate) const FALSE: Expr = 0;
pub(crate) const TRUE: Expr = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum GeometryPart {
    FarPayload,
    NearPayload,
    Head,
    RetractedBase,
    MovingBase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Variable {
    Signal {
        pos: BlockPos,
        threshold: u8,
        order: usize,
    },
    Actuator(usize),
    Memory(usize),
    Geometry {
        actor: usize,
        part: GeometryPart,
    },
}

impl Ord for Variable {
    fn cmp(&self, other: &Self) -> Ordering {
        match (*self, *other) {
            (Self::Geometry { actor: a, part: ap }, Self::Geometry { actor: b, part: bp }) => {
                (a, ap).cmp(&(b, bp))
            }
            (Self::Geometry { .. }, _) => Ordering::Greater,
            (_, Self::Geometry { .. }) => Ordering::Less,
            (
                Self::Signal {
                    pos: a,
                    threshold: at,
                    order: ao,
                },
                Self::Signal {
                    pos: b,
                    threshold: bt,
                    order: bo,
                },
            ) => (ao, at, a.y, a.z, a.x).cmp(&(bo, bt, b.y, b.z, b.x)),
            (Self::Actuator(a), Self::Actuator(b)) => a.cmp(&b),
            (Self::Memory(a), Self::Memory(b)) => a.cmp(&b),
            (Self::Memory(_), Self::Actuator(_)) => Ordering::Less,
            (Self::Actuator(_), Self::Memory(_)) => Ordering::Greater,
            (Self::Signal { .. }, Self::Memory(_)) => Ordering::Less,
            (Self::Memory(_), Self::Signal { .. }) => Ordering::Greater,
            (Self::Signal { .. }, Self::Actuator(_)) => Ordering::Less,
            (Self::Actuator(_), Self::Signal { .. }) => Ordering::Greater,
        }
    }
}
impl PartialOrd for Variable {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Decision {
    pub variable: Variable,
    pub low: Expr,
    pub high: Expr,
}

#[derive(Debug, Default)]
pub(crate) struct BooleanArena {
    pub nodes: Vec<Decision>,
    unique: FxHashMap<Decision, Expr>,
    conjunctions: FxHashMap<(Expr, Expr), Expr>,
    inverses: FxHashMap<Expr, Expr>,
    pub exhausted: bool,
    budget_multiplier: usize,
}

const MAX_DECISIONS: usize = 1_048_576;

impl BooleanArena {
    pub fn with_budget(multiplier: usize) -> Self {
        Self {
            budget_multiplier: multiplier.clamp(1, 8),
            ..Self::default()
        }
    }

    fn decision_limit(&self) -> usize {
        MAX_DECISIONS * self.budget_multiplier.clamp(1, 8)
    }

    pub fn decision(&self, id: Expr) -> Option<Decision> {
        id.checked_sub(2).map(|id| self.nodes[id as usize])
    }

    pub fn make(&mut self, variable: Variable, low: Expr, high: Expr) -> Expr {
        if low == high {
            return low;
        }
        let decision = Decision {
            variable,
            low,
            high,
        };
        if let Some(&id) = self.unique.get(&decision) {
            return id;
        }
        if self.nodes.len() >= self.decision_limit() {
            self.exhausted = true;
            return FALSE;
        }
        let id = self.nodes.len() as u32 + 2;
        self.nodes.push(decision);
        self.unique.insert(decision, id);
        id
    }

    pub fn variable(&mut self, variable: Variable) -> Expr {
        self.make(variable, FALSE, TRUE)
    }

    pub fn not(&mut self, value: Expr) -> Expr {
        if self.exhausted {
            return FALSE;
        }
        if value <= TRUE {
            return TRUE - value;
        }
        if let Some(&result) = self.inverses.get(&value) {
            return result;
        }
        let decision = self.decision(value).unwrap();
        let low = self.not(decision.low);
        let high = self.not(decision.high);
        let result = self.make(decision.variable, low, high);
        self.inverses.insert(value, result);
        self.inverses.insert(result, value);
        result
    }

    pub fn and(&mut self, a: Expr, b: Expr) -> Expr {
        if self.exhausted || self.conjunctions.len() >= self.decision_limit() * 2 {
            self.exhausted = true;
            return FALSE;
        }
        if a == FALSE || b == FALSE {
            return FALSE;
        }
        if a == TRUE || a == b {
            return b;
        }
        if b == TRUE {
            return a;
        }
        let key = if a < b { (a, b) } else { (b, a) };
        if let Some(&result) = self.conjunctions.get(&key) {
            return result;
        }
        let left = self.decision(a).unwrap();
        let right = self.decision(b).unwrap();
        let variable = left.variable.min(right.variable);
        let (al, ah) = if left.variable == variable {
            (left.low, left.high)
        } else {
            (a, a)
        };
        let (bl, bh) = if right.variable == variable {
            (right.low, right.high)
        } else {
            (b, b)
        };
        let low = self.and(al, bl);
        let high = self.and(ah, bh);
        let result = self.make(variable, low, high);
        self.conjunctions.insert(key, result);
        result
    }

    pub fn or(&mut self, a: Expr, b: Expr) -> Expr {
        let a = self.not(a);
        let b = self.not(b);
        let both = self.and(a, b);
        self.not(both)
    }

    pub fn select(&mut self, condition: Expr, yes: Expr, no: Expr) -> Expr {
        let yes = self.and(condition, yes);
        let inverse = self.not(condition);
        let no = self.and(inverse, no);
        self.or(yes, no)
    }

    pub fn evaluate(&self, root: Expr, mut read: impl FnMut(Variable) -> bool) -> bool {
        let mut id = root;
        while let Some(decision) = self.decision(id) {
            id = if read(decision.variable) {
                decision.high
            } else {
                decision.low
            };
        }
        id == TRUE
    }

    /// Replace already-resolved actuator variables with pure port functions.
    /// Rebuilding through select keeps the variable order canonical.
    pub fn substitute(&mut self, root: Expr, resolved: &[Option<Expr>]) -> Expr {
        fn visit(
            arena: &mut BooleanArena,
            id: Expr,
            resolved: &[Option<Expr>],
            memo: &mut FxHashMap<Expr, Expr>,
        ) -> Expr {
            let Some(decision) = arena.decision(id) else {
                return id;
            };
            if let Some(&result) = memo.get(&id) {
                return result;
            }
            let low = visit(arena, decision.low, resolved, memo);
            let high = visit(arena, decision.high, resolved, memo);
            let variable = match decision.variable {
                Variable::Actuator(actor) => {
                    resolved[actor].expect("dependency order checked before substitution")
                }
                variable => arena.variable(variable),
            };
            let result = arena.select(variable, high, low);
            memo.insert(id, result);
            result
        }
        visit(self, root, resolved, &mut FxHashMap::default())
    }

    pub fn actuator_dependencies(&self, root: Expr) -> Vec<usize> {
        let mut stack = vec![root];
        let mut visited = rustc_hash::FxHashSet::default();
        let mut actors = Vec::new();
        while let Some(id) = stack.pop() {
            if !visited.insert(id) {
                continue;
            }
            if let Some(decision) = self.decision(id) {
                if let Variable::Actuator(actor) = decision.variable {
                    actors.push(actor);
                }
                stack.extend([decision.low, decision.high]);
            }
        }
        actors.sort_unstable();
        actors.dedup();
        actors
    }

    /// Discard provisional actuator decisions and apply caches. Only decisions
    /// reachable from final response roots are kept by the backend.
    pub fn compact(&self, roots: &mut [Expr]) -> Self {
        fn visit(
            old: &BooleanArena,
            new: &mut BooleanArena,
            id: Expr,
            memo: &mut FxHashMap<Expr, Expr>,
        ) -> Expr {
            let Some(d) = old.decision(id) else {
                return id;
            };
            if let Some(&result) = memo.get(&id) {
                return result;
            }
            let low = visit(old, new, d.low, memo);
            let high = visit(old, new, d.high, memo);
            let result = new.make(d.variable, low, high);
            memo.insert(id, result);
            result
        }
        let mut result = Self::with_budget(self.budget_multiplier);
        let mut memo = FxHashMap::default();
        for root in roots {
            *root = visit(self, &mut result, *root, &mut memo);
        }
        result.unique = FxHashMap::default();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(order: usize) -> Variable {
        Variable::Signal {
            pos: BlockPos::new(order as i32, 0, 0),
            threshold: 0,
            order,
        }
    }

    #[test]
    fn complementary_paths_remove_spurious_actuator_dependencies() {
        let mut arena = BooleanArena::default();
        let data = arena.variable(signal(0));
        let actor = arena.variable(Variable::Actuator(0));
        let inverse = arena.not(actor);
        let first = arena.and(data, actor);
        let second = arena.and(data, inverse);
        let result = arena.or(first, second);
        assert_eq!(result, data);
        assert!(arena.actuator_dependencies(result).is_empty());
    }

    #[test]
    fn actuator_composition_and_compaction_preserve_truth_tables() {
        let mut arena = BooleanArena::default();
        let a = arena.variable(signal(0));
        let b = arena.variable(signal(1));
        let actor = arena.variable(Variable::Actuator(0));
        let inverse = arena.not(a);
        let root = arena.select(actor, inverse, a);
        let not_b = arena.not(b);
        let composed = arena.substitute(root, &[Some(not_b)]);
        assert!(arena.actuator_dependencies(composed).is_empty());
        let mut roots = [composed];
        let compact = arena.compact(&mut roots);
        assert!(compact.nodes.len() < arena.nodes.len());
        for av in [false, true] {
            for bv in [false, true] {
                let read = |variable| match variable {
                    Variable::Signal { order: 0, .. } => av,
                    Variable::Signal { order: 1, .. } => bv,
                    _ => panic!("unresolved variable"),
                };
                assert_eq!(arena.evaluate(composed, read), av ^ !bv);
                assert_eq!(compact.evaluate(roots[0], read), av ^ !bv);
            }
        }
    }
}
