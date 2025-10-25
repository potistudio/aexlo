pub fn hello() {
	println!("Hello from the wrapper library!");
}

pub enum Command {
	About,
	GlobalSetup,
	ParamSetup,
	Render,
}

mod layer;
mod pixel;

pub use layer::*;
pub use pixel::*;
