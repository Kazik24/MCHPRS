use super::*;

include!("tests/shared_pistons.rs");
include!("tests/clock_and_piston_probes.rs");
include!("tests/netlist.rs");

#[path = "tests/real_circuits.rs"]
mod real_circuits;
