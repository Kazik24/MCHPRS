//! Compiler-only guarded work; selected occurrences retain native FIFO and pending membership.
use super::*;
use crate::redpiler::backend::{NUM_QUEUES, TickScheduler};
use mchprs_world::{PistonAction, TickPriority};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Request {
    /// This identifies the logical actuator to revalidate when this request is consumed.
    pub(crate) actor: PistonId,
    /// This preserves the distinction between extending, pulling and early non-pulling retraction.
    pub(crate) action: PistonAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Guarded<T> {
    /// This selects exactly the input valuations under which this occurrence exists.
    pub(crate) when: Formula,
    /// This is the typed request or scheduled node, without a concrete input snapshot.
    pub(crate) value: T,
}

pub(crate) struct Work {
    /// These guarded requests preserve first-occurrence FIFO order independently for each valuation.
    pub(crate) requests: VecDeque<Guarded<Request>>,
    /// This reuses the native ring/priority ordering for compiler-only scheduled occurrences.
    pub(crate) scheduler: TickScheduler<Guarded<NodeId>>,
    /// These predicates describe actual matching scheduler membership, not component display flags.
    pub(crate) pending: Vec<Formula>,
}

impl Work {
    /// Imported work needs explicit symbolic admission; this constructor admits an empty queue only.
    pub(crate) fn empty(node_count: usize) -> Self {
        Self {
            requests: VecDeque::new(),
            scheduler: TickScheduler::default(),
            pending: vec![Formula::FALSE; node_count],
        }
    }

    pub(crate) fn request(&mut self, compiler: &mut Compiler<'_>, value: Request, when: Formula) -> Result<Formula> {
        compiler.step()?;
        let mut queued = Formula::FALSE;
        for occurrence in &self.requests {
            if occurrence.value == value {
                queued = compiler.logic.or(queued, occurrence.when)?;
            }
        }
        let absent = compiler.logic.not(queued)?;
        let active = compiler.logic.and(when, absent)?;
        if active != Formula::FALSE {
            self.requests.push_back(Guarded { value, when: active });
        }
        Ok(active)
    }

    pub(crate) fn schedule(
        &mut self,
        compiler: &mut Compiler<'_>,
        node: NodeId,
        delay: usize,
        priority: TickPriority,
        when: Formula,
    ) -> Result<Formula> {
        compiler.step()?;
        if delay >= NUM_QUEUES || (delay == 0 && priority != TickPriority::NanoTick) {
            return Ok(Formula::FALSE);
        }
        let absent = compiler.logic.not(self.pending[node.index()])?;
        let active = compiler.logic.and(when, absent)?;
        if active != Formula::FALSE {
            self.scheduler.schedule_half_tick(Guarded { when: active, value: node }, delay, priority);
            self.pending[node.index()] = compiler.logic.or(self.pending[node.index()], active)?;
        }
        Ok(active)
    }

    /// Remove membership before compiling the handler, so it may schedule itself again.
    pub(crate) fn pop_tick(&mut self, compiler: &mut Compiler<'_>) -> Result<Option<Guarded<NodeId>>> {
        let Some(occurrence) = self.scheduler.this_tick().pop_first() else {
            return Ok(None);
        };
        compiler.step()?;
        let retain = compiler.logic.not(occurrence.when)?;
        self.pending[occurrence.value.index()] = compiler.logic.and(self.pending[occurrence.value.index()], retain)?;
        Ok(Some(occurrence))
    }
}
