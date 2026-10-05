#![deny(rust_2018_idioms)]

#[macro_use]
mod utils;
mod chat;
mod chat_commands;
mod config;
mod container;
mod interaction;
mod messages;
mod permissions;
mod player;
pub mod plot;
mod profile;
mod proxy_chat;
pub mod redpiler;
pub mod redstone;
pub mod server;
mod sound;
mod velocity;
pub mod world;

#[macro_use]
extern crate bitflags;

#[cfg(test)]
mod tests;
