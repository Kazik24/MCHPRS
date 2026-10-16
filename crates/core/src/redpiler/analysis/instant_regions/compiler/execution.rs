//! Compile the graph's ordered event semantics into symbolic values and conditional effects.
mod components;
mod geometry;
mod pistons;
use super::*;
use crate::redpiler::components::NodeType;
use mchprs_world::{PistonAction, TickPriority};
use std::collections::VecDeque;
pub(crate) use work::{Guarded, Request};
use work::Work;

#[derive(Clone)]
pub(crate) struct ResourcePublication {
    /// This selects the valuations where an accepted action actually transferred this payload.
    pub(crate) when: Formula,
    /// This is the shared payload identity whose publication must be revalidated.
    pub(crate) resource: ResourceId,
    /// This records the revision-write position immediately after the transfer.
    pub(crate) stamp: usize,
    /// This is the contact where the pending payload was placed.
    pub(crate) contact: ContactId,
}

#[derive(Clone)]
pub(crate) struct Publication {
    /// This selects the valuations where the actor accepted the action.
    pub(crate) when: Formula,
    /// This independently validates the actor transition without suppressing payload publication.
    pub(crate) actor_valid: Formula,
    /// This is the actuator whose stationary connections will be restored.
    pub(crate) actor: PistonId,
    /// This records the actor revision-write position after acceptance.
    pub(crate) stamp: usize,
    /// This retains the independently validated payload publication, if the action transferred one.
    pub(crate) resource: Option<ResourcePublication>,
    /// This is the exact compiler-phase deadline in logical ticks.
    pub(crate) due_tick: u64,
    /// This distinguishes head publication from base publication and preserves their route order.
    pub(crate) extending: bool,
}

#[derive(Clone)]
pub(crate) struct Accepted {
    /// This selects the valuations in which this ordered action occurs.
    pub(crate) when: Formula,
    /// This retains actor identity and the precise accepted action kind.
    pub(crate) request: Request,
    /// This freezes the extension bit required by the externally observable block action.
    pub(crate) previous_extended: Formula,
    /// This preserves the phase deadline of the action effect.
    pub(crate) tick: u64,
}

#[derive(Clone)]
pub(crate) enum Effect {
    /// This schedules one node through the existing half-tick priority queues.
    Schedule {
        /// This selects inputs where the schedule is newly inserted.
        when: Formula,
        /// This is the scheduled component identity.
        node: NodeId,
        /// This preserves the exact native half-tick delay.
        delay: usize,
        /// This preserves the native ordering within a scheduler bucket.
        priority: TickPriority,
    },
    /// This appends one action request after preserving current FIFO deduplication.
    Request {
        /// This selects inputs where this request is newly inserted.
        when: Formula,
        /// This is the action owner and exact requested action.
        request: Request,
    },
    /// This creates one deferred actor/resource publication witness.
    Publication(Publication),
    /// This records one accepted-action presentation occurrence.
    Accepted(Accepted),
    /// This dispatches one outside callback with the sparse owned-state snapshot it observes.
    Callback {
        /// This retains target, direction, payload and occurrence identity for native dispatch.
        delivery: Delivery,
        /// These changed owned fields reproduce live reads at this exact event occurrence.
        snapshot: Vec<(RegionStateSlot, Formula)>,
    },
}

pub(crate) struct Execution {
    /// These guarded queues retain scheduled work and request deduplication during compilation only.
    pub(crate) work: Work,
    /// These pending publications retain independent actor and shared-resource witnesses.
    pub(crate) publications: VecDeque<Publication>,
    /// These ordered action effects remain roots until boundary liveness is determined.
    pub(crate) accepted: Vec<Accepted>,
    /// This records the logical tick being compiled, without physical motion or runtime learning.
    pub(crate) tick: u64,
    /// This marks the scheduled/action phases where a due extension still counts as an early drop.
    early_phase: bool,
    /// This limits recursive callback execution to one region when compiling a local event routine.
    scope: Option<RegionScope>,
    /// These outside deliveries are boundary roots instead of recursively executed components.
    pub(crate) outgoing: Vec<Delivery>,
}

impl Execution {
    /// The first entry family admits no imported work; later admission must supply it explicitly.
    pub(crate) fn empty(graph: &CompileGraph) -> Self {
        Self {
            work: Work::empty(graph.nodes.len()),
            publications: VecDeque::new(),
            accepted: Vec::new(),
            tick: 0,
            early_phase: false,
            scope: None,
            outgoing: Vec::new(),
        }
    }

    /// Build one callback cascade that stops at the region boundary and retains its outside effects.
    pub(crate) fn for_region(
        compiler: &mut Compiler<'_>,
        region: &crate::redpiler::program_graph::InstantRegion,
    ) -> Result<Self> {
        let mut execution = Self::empty(compiler.graph);
        execution.scope = Some(RegionScope::new(compiler.graph, region));
        for &node in &region.resets {
            execution.work.pending[node.index()] = compiler.pending_input(node)?;
        }
        Ok(execution)
    }

    pub(crate) fn run_tick(&mut self, compiler: &mut Compiler<'_>) -> Result<()> {
        compiler.step()?;
        self.tick = self.tick.checked_add(1).ok_or(SymbolicError::BudgetExceeded)?;
        self.early_phase = true;
        self.drain_ticks(compiler)?;
        self.work.scheduler.end_last_tick_move_next();
        self.drain_ticks(compiler)?;
        while let Some(request) = self.work.requests.pop_front() {
            compiler.step()?;
            self.accept(compiler, request)?;
        }
        self.early_phase = false;
        // Native publication takes the due prefix before any restoration callback can append work.
        let due = self.publications.iter().take_while(|event| event.due_tick <= self.tick).count();
        for _ in 0..due {
            compiler.step()?;
            let event = self.publications.pop_front().unwrap();
            self.publish(compiler, event)?;
        }
        Ok(())
    }

    fn drain_ticks(&mut self, compiler: &mut Compiler<'_>) -> Result<()> {
        while let Some(event) = self.work.pop_tick(compiler)? {
            self.tick_component(compiler, event.value, event.when)?;
        }
        Ok(())
    }

    pub(crate) fn set_control(&mut self, compiler: &mut Compiler<'_>, node: NodeId, powered: Formula) -> Result<()> {
        if !matches!(
            compiler.graph.nodes[node.index()].kind,
            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::PressurePlate)
        ) {
            return Err(CompileError::UnsupportedNode(node));
        }
        let strength = compiler.logic.signal_select(powered, Signal::literal(15)?, Signal::literal(0)?)?;
        self.set_component(compiler, node, Formula::TRUE, powered, strength, None, true)
    }

    fn fire(&mut self, compiler: &mut Compiler<'_>, node: NodeId, when: Formula) -> Result<()> {
        if let Some(route) = compiler.graph.nodes[node.index()].output_route {
            compiler.route(route, when, None, self)?;
        }
        Ok(())
    }
}

impl Receivers for Execution {
    fn deliver(&mut self, compiler: &mut Compiler<'_>, delivery: Delivery) -> Result<()> {
        if delivery.when == Formula::FALSE {
            return Ok(());
        }
        if self.scope.as_ref().is_some_and(|scope| !scope.owns(delivery.receiver)) {
            compiler.record_effect(Effect::Callback {
                delivery: delivery.clone(),
                snapshot: Vec::new(),
            })?;
            self.outgoing.push(delivery);
            return Ok(());
        }
        compiler.step()?;
        match delivery.receiver {
            CallbackReceiver::Component(node) => self.update_component(compiler, node, delivery.kind, delivery.when, delivery.captured.as_ref()),
            CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => self.sample(compiler, actor, delivery.when),
            CallbackReceiver::Contact(_) => Ok(()),
        }
    }
}
