#![feature(c_variadic)]

#[macro_use]
extern crate dlopen_derive;

mod diagnostics;
mod plugin_instance;

pub use diagnostics::*;
pub use plugin_instance::*;
