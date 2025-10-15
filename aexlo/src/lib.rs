//! AEXLO - After Effects Plugin Framework for Rust

#![feature(c_variadic)]
#![warn(clippy::all)]

#[macro_use]
extern crate dlopen_derive;

mod diagnostics;
mod plugin_instance;

pub use diagnostics::*;
pub use plugin_instance::*;
