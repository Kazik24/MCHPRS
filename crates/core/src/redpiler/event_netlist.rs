//! Offline EventProgram compiler and executor. Not installed in the Graph compiler.
#[path = "event_netlist/compiler.rs"]
pub(crate) mod compiler;
#[path = "event_netlist/executor.rs"]
pub(crate) mod executor;
#[path = "event_netlist/program.rs"]
pub(crate) mod program;
#[path = "event_netlist/scratch.rs"]
pub(crate) mod scratch;

#[cfg(test)]
#[path = "event_netlist/tests.rs"]
mod tests;
