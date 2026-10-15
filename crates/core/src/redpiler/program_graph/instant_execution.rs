//! Lower held-input kernels to unconditional phase effects, leaving reconstruction cold.
//! Boolean roots select occurrences, never reorder them; scalar values remain exact u64 roots.
//! shortcut: enumerate only the certified three-input family; use symbolic plans before admitting larger interfaces.
use super::*;
use mchprs_redpiler_opt::{Circuit, Limits, Op};
use mchprs_world::PistonAction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KernelExecution {
    /// This identifies the start of the common repeated phase suffix.
    pub(crate) prefix: usize,
    /// This is the original phase count used by the semantic reconstruction cursor.
    pub(crate) phase_count: usize,
    /// These are shared unconditional effects for distinct held-input phases.
    phases: Box<[KernelEffects]>,
    /// This maps input-mask/semantic-phase pairs to their shared effect list.
    dispatch: Box<[u16]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KernelEffects {
    /// These are active Electrical owner writes with already evaluated packed values.
    pub(crate) writes: Box<[(NodeId, u16)]>,
    /// These owners were touched in this phase, including same-value writes.
    pub(crate) dirty: Box<[NodeId]>,
    /// These ordered presentation occurrences retain actor, action and captured extension flag.
    pub(crate) effects: Box<[(PistonId, PistonAction, bool)]>,
    /// This is the exact logical scheduled-work count discharged by the phase.
    pub(crate) scheduled: u64,
    /// This is the exact logical connection-publication count for the phase.
    pub(crate) published: u64,
}

impl KernelExecution {
    /// Called only after the graph's kernel certificate has been validated.
    pub(crate) fn compile(kernel: &InstantKernel) -> Result<Self, GraphBuildError> {
        let invalid = || GraphBuildError::InvalidInstantMachine { state: 0, action: 0 };
        let circuit = condition_circuit(kernel).ok_or_else(invalid)?;
        let mut phases = Vec::new();
        let mut dispatch = Vec::new();
        for mask in 0..(1u8 << kernel.data_controls.len()) {
            let inputs: Vec<_> = (0..kernel.data_controls.len()).map(|bit| mask & (1 << bit) != 0).collect();
            let bindings: Vec<_> = inputs.iter().copied().map(Some).collect();
            let plan = mchprs_redpiler_opt::specialize(&circuit, &bindings, Limits::default(), || false).map_err(|_| invalid())?;
            let mut selected = vec![false; plan.roots().len()];
            plan.evaluate(&inputs, &mut vec![false; plan.scratch_len()], &mut selected)
                .map_err(|_| invalid())?;
            let mut selected = selected.into_iter();
            for phase in &kernel.phases {
                let effects = specialize_phase(kernel, phase, mask, &mut selected).ok_or_else(invalid)?;
                // There are at most eight input masks and 128 phases; sharing is compile-time only.
                let index = match phases.iter().position(|existing| existing == &effects) {
                    Some(index) => index,
                    None => {
                        phases.push(effects);
                        phases.len() - 1
                    }
                };
                dispatch.push(u16::try_from(index).map_err(|_| invalid())?);
            }
            if selected.next().is_some() {
                return Err(invalid());
            }
        }
        Ok(Self {
            prefix: usize::from(kernel.prefix),
            phase_count: kernel.phases.len(),
            phases: phases.into_boxed_slice(),
            dispatch: dispatch.into_boxed_slice(),
        })
    }

    pub(crate) fn phase(&self, mask: u8, phase: usize) -> Option<&KernelEffects> {
        if phase >= self.phase_count {
            return None;
        }
        let index = *self.dispatch.get(usize::from(mask) * self.phase_count + phase)?;
        self.phases.get(usize::from(index))
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        std::mem::size_of_val(self.dispatch.as_ref())
            + std::mem::size_of_val(self.phases.as_ref())
            + self
                .phases
                .iter()
                .map(|phase| {
                    std::mem::size_of_val(phase.writes.as_ref())
                        + std::mem::size_of_val(phase.dirty.as_ref())
                        + std::mem::size_of_val(phase.effects.as_ref())
                })
                .sum::<usize>()
    }
}

fn condition_circuit(kernel: &InstantKernel) -> Option<Circuit> {
    let count = kernel.data_controls.len();
    let mut ops: Vec<_> = (0..count).map(Op::Input).collect();
    let mut translated = Vec::with_capacity(kernel.expressions.len());
    for expression in &kernel.expressions {
        let op = match *expression {
            KernelExpr::Constant(0) => Some(Op::Const(false)),
            KernelExpr::Constant(1) => Some(Op::Const(true)),
            KernelExpr::Constant(_) => None,
            KernelExpr::Select { control, low, high } => match (translated[low.0 as usize], translated[high.0 as usize]) {
                (Some(low), Some(high)) => Some(Op::Select(usize::from(control), high, low)),
                _ => None,
            },
        };
        translated.push(op.map(|op| {
            let id = ops.len();
            ops.push(op);
            id
        }));
    }
    let mut roots = Vec::new();
    for phase in &kernel.phases {
        for root in phase
            .components
            .iter()
            .map(|delta| delta.changed)
            .chain(phase.dirty.iter().map(|&(_, root)| root))
            .chain(phase.effects.iter().map(|effect| effect.enabled))
        {
            roots.push(translated[root.0 as usize]?);
        }
    }
    Some(Circuit::new((0..count).collect(), ops, roots))
}

fn specialize_phase(kernel: &InstantKernel, phase: &KernelPhase, mask: u8, selected: &mut impl Iterator<Item = bool>) -> Option<KernelEffects> {
    let mut writes = Vec::new();
    for delta in &phase.components {
        if selected.next()? {
            writes.push((delta.node, u16::try_from(kernel.evaluate(delta.value, mask)).ok()?));
        }
    }
    let mut dirty = Vec::new();
    for &(node, _) in &phase.dirty {
        if selected.next()? {
            dirty.push(node);
        }
    }
    let mut effects = Vec::new();
    for effect in &phase.effects {
        if selected.next()? {
            effects.push((effect.piston, effect.action, effect.extended));
        }
    }
    if effects.len() as u64 != kernel.evaluate(phase.accepted, mask) {
        return None;
    }
    Some(KernelEffects {
        writes: writes.into_boxed_slice(),
        dirty: dirty.into_boxed_slice(),
        effects: effects.into_boxed_slice(),
        scheduled: kernel.evaluate(phase.scheduled, mask),
        published: kernel.evaluate(phase.published, mask),
    })
}
