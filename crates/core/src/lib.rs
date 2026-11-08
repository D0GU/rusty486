//! rusty486 core: an i486 CPU plus the fantasy PC's bus and devices.
//!
//! This crate does no host I/O (no files, windows, audio or threads) so it can
//! be compiled to WebAssembly. Frontends feed it disk images as byte vectors
//! and read the screen out of emulated memory.

pub mod bus;
pub mod cpu;
pub mod devices;
pub mod fat;
pub mod machine;

pub use bus::Bus;
pub use cpu::Cpu;
pub use machine::{Machine, PcBus};
