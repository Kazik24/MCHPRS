//! Canonical Boolean functions used by geometric wave extraction. Node zero
//! and one are terminals; every other node is an ordered decision on a port
//! threshold or a provisional actuator response.
use mchprs_blocks::BlockPos;
use rustc_hash::FxHashMap;
use std::cmp::{Ordering, Reverse};

pub(crate) type Expr = u32;
pub(crate) const FALSE: Expr = 0;
pub(crate) const TRUE: Expr = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Variable {
    Signal { pos: BlockPos, threshold: u8 },
    Actuator(usize),
}

impl Ord for Variable {
    fn cmp(&self, other: &Self) -> Ordering {
        match (*self, *other) {
            (
                Self::Signal {
                    pos: a,
                    threshold: at,
                },
                Self::Signal {
                    pos: b,
                    threshold: bt,
                },
            ) => (Reverse(a.z), a.y, a.x, at).cmp(&(Reverse(b.z), b.y, b.x, bt)),
            (Self::Actuator(a), Self::Actuator(b)) => a.cmp(&b),
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
}

impl BooleanArena {
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
        let id = self.nodes.len() as u32 + 2;
        self.nodes.push(decision);
        self.unique.insert(decision, id);
        id
    }

    pub fn variable(&mut self, variable: Variable) -> Expr {
        self.make(variable, FALSE, TRUE)
    }

    pub fn not(&mut self, value: Expr) -> Expr {
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
}
