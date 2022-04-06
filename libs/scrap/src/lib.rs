#[macro_use]
extern crate cfg_if;
pub extern crate libc;

pub use common::*;

#[cfg(x11)]
pub mod x11;

#[cfg(all(x11, feature="wayland"))]
pub mod wayland;

mod common;
