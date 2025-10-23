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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn layer_basic_ops() {
		let w = 4;
		let h = 3;
		let mut pixels = Vec::with_capacity((w * h) as usize);
		for _ in 0..(w * h) {
			pixels.push(Pixel::<Depth8>::white());
		}

		let mut layer = Layer::from_vec(w, h, pixels);
		assert_eq!(layer.width(), w);
		assert_eq!(layer.height(), h);
		assert_eq!(layer.len(), (w * h) as usize);

		// Indexing via at and Index trait
		let p = layer.at(1, 2);
		assert_eq!(p.alpha, 255);

		// Mutable set via at_mut
		let px = layer.at_mut(0, 0);
		*px = Pixel::<Depth8>::black();
		assert_eq!(layer[(0, 0)].alpha, 255);

		// Iteration
		let count = layer.iter().count();
		assert_eq!(count, layer.len());
	}

	#[test]
	#[should_panic]
	fn out_of_bounds_panics() {
		let layer = Layer::<Depth8>::blank(2, 2);
		// This should panic
		let _ = layer.at(3, 0);
	}
}
