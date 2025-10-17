//! AEXLO - After Effects Plugin Framework for Rust

#![feature(c_variadic)]
#![warn(clippy::all)]
#![allow(non_snake_case)]

#[macro_use]
extern crate dlopen_derive;

mod diagnostics;
mod plugin_instance;
mod ansi;

pub use diagnostics::*;
pub use plugin_instance::*;
pub use after_effects_sys::PF_Pixel;
