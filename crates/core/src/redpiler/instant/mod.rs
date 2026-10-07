//! Contracts at the boundary between ordinary redstone and instant regions.
//!
//! Recognition and execution are separate: declaring a port never certifies a
//! physical circuit, and observing a falling edge never moves a piston.
pub(crate) mod boolean;
pub(crate) mod boundary;
pub(crate) mod clocked;
pub mod contract;
pub(crate) mod logic;
pub(crate) mod outputs;
pub(crate) mod observer;
pub(crate) mod program;
pub(crate) mod regions;
pub(crate) mod sequential;
