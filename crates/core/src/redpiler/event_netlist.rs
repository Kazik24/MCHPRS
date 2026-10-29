//! Experimental, isolated event-wave runtime. Not installed in the Graph compiler.
use std::collections::VecDeque;

#[derive(Clone, Copy)]
pub(crate) struct Link {
    pub source: usize,
    pub target: usize,
    pub attenuation: u8,
    pub contact: Option<(usize, usize)>,
}

#[derive(Clone, Copy)]
pub(crate) struct Action {
    pub input: usize,
    pub threshold: u8,
    pub when_powered: bool,
    pub resource: usize,
    pub from: usize,
    pub to: usize,
}

// shortcut: links read external sources only, extend after graph-derived event routing exists.
#[derive(Clone)]
pub(crate) struct EventNetlist {
    sources: Vec<u8>,
    binary_sources: bool,
    contacts: Vec<usize>,
    links: Vec<Link>,
    actions: Vec<Action>,
    incoming: Vec<Vec<usize>>,
    contact_watchers: Vec<Vec<usize>>,
    cycle_consumed: Option<Vec<bool>>,
}

#[path = "event_netlist/autonomous_piston.rs"]
mod autonomous_piston;
#[path = "event_netlist/compiled_pistons.rs"]
mod compiled_pistons;
#[path = "event_netlist/compiler.rs"]
pub(crate) mod compiler;
#[path = "event_netlist/executor.rs"]
pub(crate) mod executor;
#[path = "event_netlist/netlist.rs"]
mod netlist;
#[path = "event_netlist/observed_probes.rs"]
mod observed_probes;
#[path = "event_netlist/physical_piston.rs"]
mod physical_piston;
#[path = "event_netlist/program.rs"]
pub(crate) mod program;
#[path = "event_netlist/scratch.rs"]
mod scratch;

use autonomous_piston::*;
pub(crate) use compiled_pistons::CompiledPistonActors;
use physical_piston::*;

#[cfg(test)]
#[path = "event_netlist/tests.rs"]
mod tests;
