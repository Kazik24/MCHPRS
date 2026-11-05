#![deny(rust_2018_idioms)]

#[cfg(test)]
extern crate self as mchprs_core;

#[macro_use]
mod utils;
mod chat;
mod chat_commands;
#[cfg(test)]
#[path = "tests/circuit_geometry.rs"]
mod circuit_geometry;
mod config;
mod container;
pub mod headless;
mod interaction;
mod messages;
mod permissions;
mod player;
pub mod plot;
mod profile;
mod redbox;
pub mod redpilator;
pub mod redstone;
pub mod server;
mod signed_velocity;
mod sound;
mod velocity;
mod waveform;
pub mod world;

#[macro_use]
extern crate bitflags;

#[cfg(test)]
mod tests;
