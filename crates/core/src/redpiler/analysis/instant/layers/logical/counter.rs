//! Bounded symbolic proof for controlled binary word transitions.
use super::*;
use rustc_hash::FxHashMap;
#[cfg(test)]
#[path = "bdd_tests.rs"]
mod bdd_tests;

const MAX_BDD_NODES: usize = 1_000_000;
const MAX_BDD_WORK: usize = 8_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum BddOp {
    And,
    Or,
}

#[derive(Clone, Copy)]
struct BddNode {
    variable: u16,
    low: u32,
    high: u32,
}

#[derive(Default)]
struct BddManager {
    nodes: Vec<BddNode>,
    unique: FxHashMap<(u16, u32, u32), u32>,
    apply_cache: FxHashMap<(BddOp, u32, u32), u32>,
    not_cache: FxHashMap<u32, u32>,
    work: usize,
}

impl BddManager {
    fn variable(&mut self, variable: u16) -> Result<u32, ConstructionError> {
        self.make_node(variable, 0, 1)
    }

    fn make_node(&mut self, variable: u16, low: u32, high: u32) -> Result<u32, ConstructionError> {
        if low == high {
            return Ok(low);
        }
        if let Some(&node) = self.unique.get(&(variable, low, high)) {
            return Ok(node);
        }
        if self.nodes.len() >= MAX_BDD_NODES || self.work >= MAX_BDD_WORK {
            return Err(ConstructionError::BudgetExceeded);
        }
        let id = u32::try_from(self.nodes.len() + 2).map_err(|_| ConstructionError::BudgetExceeded)?;
        self.nodes.push(BddNode { variable, low, high });
        self.unique.insert((variable, low, high), id);
        Ok(id)
    }

    fn negate(&mut self, value: u32) -> Result<u32, ConstructionError> {
        match value {
            0 => return Ok(1),
            1 => return Ok(0),
            _ => {}
        }
        if let Some(&result) = self.not_cache.get(&value) {
            return Ok(result);
        }
        self.work += 1;
        if self.work >= MAX_BDD_WORK {
            return Err(ConstructionError::BudgetExceeded);
        }
        let node = self.nodes[value as usize - 2];
        let low = self.negate(node.low)?;
        let high = self.negate(node.high)?;
        let result = self.make_node(node.variable, low, high)?;
        self.not_cache.insert(value, result);
        Ok(result)
    }

    fn and(&mut self, left: u32, right: u32) -> Result<u32, ConstructionError> {
        self.apply(BddOp::And, left, right)
    }

    fn or(&mut self, left: u32, right: u32) -> Result<u32, ConstructionError> {
        self.apply(BddOp::Or, left, right)
    }

    fn xor(&mut self, left: u32, right: u32) -> Result<u32, ConstructionError> {
        let not_left = self.negate(left)?;
        let not_right = self.negate(right)?;
        let positive = self.and(left, not_right)?;
        let negative = self.and(not_left, right)?;
        self.or(positive, negative)
    }

    fn apply(&mut self, op: BddOp, mut left: u32, mut right: u32) -> Result<u32, ConstructionError> {
        if left > right {
            std::mem::swap(&mut left, &mut right);
        }
        match op {
            BddOp::And if left == 0 || right == 0 => return Ok(0),
            BddOp::And if left == 1 => return Ok(right),
            BddOp::And if left == right => return Ok(left),
            BddOp::Or if right == 1 || left == 1 => return Ok(1),
            BddOp::Or if left == 0 => return Ok(right),
            BddOp::Or if left == right => return Ok(left),
            _ => {}
        }
        if let Some(&result) = self.apply_cache.get(&(op, left, right)) {
            return Ok(result);
        }
        self.work += 1;
        if self.work >= MAX_BDD_WORK {
            return Err(ConstructionError::BudgetExceeded);
        }
        let left_var = self.variable_of(left);
        let right_var = self.variable_of(right);
        let variable = left_var.min(right_var);
        let (left_low, left_high) = self.cofactors(left, variable);
        let (right_low, right_high) = self.cofactors(right, variable);
        let low = self.apply(op, left_low, right_low)?;
        let high = self.apply(op, left_high, right_high)?;
        let result = self.make_node(variable, low, high)?;
        self.apply_cache.insert((op, left, right), result);
        Ok(result)
    }

    fn variable_of(&self, node: u32) -> u16 {
        node.checked_sub(2)
            .and_then(|index| self.nodes.get(index as usize))
            .map_or(u16::MAX, |node| node.variable)
    }

    fn cofactors(&self, node: u32, variable: u16) -> (u32, u32) {
        node.checked_sub(2)
            .and_then(|index| self.nodes.get(index as usize))
            .filter(|node| node.variable == variable)
            .map_or((node, node), |node| (node.low, node.high))
    }
}

fn prove_word_transition(
    equations: &Equations<'_>,
    registers: &[(usize, BudMemoryRead, LayerExprId)],
    bit_by_resource: &FxHashMap<ResourceId, usize>,
    control: NodeId,
    monitor: &TaskMonitor,
) -> Result<(), ConstructionError> {
    let mut bdd = BddManager::default();
    let enable = bdd.variable(0)?;
    let disabled = bdd.negate(enable)?;
    let bit_vars: Vec<_> = (0..registers.len()).map(|bit| bdd.variable(bit as u16 + 1)).collect::<Result<_, _>>()?;
    let mut input_roots = FxHashMap::default();
    let mut next_unknown = registers.len() as u16 + 1;
    for input in &equations.reduced.inputs {
        if input_roots.contains_key(&input.source) {
            continue;
        }
        let root = match input.source {
            LayerInputSource::SourceAbove { site, .. } if equations.graph.sites[site.index()].source == SiteSource::Component(control) => enable,
            LayerInputSource::State(StateTest::ResourceAt { resource, contact }) if bit_by_resource.contains_key(&resource) => {
                let bit = bit_by_resource[&resource];
                let read = registers[bit].1;
                let spec = equations.graph.pistons[read.storage.index()];
                let PistonLane::SingleResource { far } = spec.lane else {
                    return Err("counter bit has non-single-resource storage geometry".into());
                };
                let other = if read.source == spec.near {
                    far
                } else if read.source == far {
                    spec.near
                } else {
                    return Err("counter bit source is outside its storage lane".into());
                };
                if contact == read.source {
                    bit_vars[bit]
                } else if contact == other {
                    bdd.negate(bit_vars[bit])?
                } else {
                    let root = bdd.variable(next_unknown)?;
                    next_unknown = next_unknown.checked_add(1).ok_or(ConstructionError::BudgetExceeded)?;
                    root
                }
            }
            _ => {
                let root = bdd.variable(next_unknown)?;
                next_unknown = next_unknown.checked_add(1).ok_or(ConstructionError::BudgetExceeded)?;
                root
            }
        };
        input_roots.insert(input.source, root);
    }

    let mut expression_roots = Vec::with_capacity(equations.reduced.expressions.len());
    for (index, expression) in equations.reduced.expressions.iter().enumerate() {
        if index & 1023 == 0 {
            check_cancelled(monitor)?;
        }
        let root = match expression {
            LayerExpr::Constant(value) => u32::from(*value),
            LayerExpr::Input(input) => input_roots[&equations.reduced.inputs[input.0 as usize].source],
            LayerExpr::Not(child) => bdd.negate(expression_roots[child.0 as usize])?,
            LayerExpr::All(children) => {
                let mut value = 1;
                for child in children.iter() {
                    value = bdd.and(value, expression_roots[child.0 as usize])?;
                }
                value
            }
            LayerExpr::Any(children) => {
                let mut value = 0;
                for child in children.iter() {
                    value = bdd.or(value, expression_roots[child.0 as usize])?;
                }
                value
            }
        };
        expression_roots.push(root);
    }

    let mut lower_bits_zero = 1;
    for (bit, (_, _, root)) in registers.iter().enumerate() {
        let active_next = if bit == 0 {
            bdd.negate(bit_vars[bit])?
        } else {
            bdd.xor(bit_vars[bit], lower_bits_zero)?
        };
        let expected = bdd.or(disabled, active_next)?;
        if expression_roots[root.0 as usize] != expected {
            return Err("storage equations are not a symbolically proven controlled unit word transition".into());
        }
        let bit_is_zero = bdd.negate(bit_vars[bit])?;
        lower_bits_zero = bdd.and(lower_bits_zero, bit_is_zero)?;
    }
    Ok(())
}
