use after_effects::{self as ae};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Params {}

#[derive(Debug, Default)]
struct Plugin {}

ae::define_effect!(Plugin, (), Params);

impl AdobePluginGlobal for Plugin {
	fn params_setup(
		&self,
		_params: &mut Parameters<Params>,
		_in_data: InData,
		_out_data: OutData,
	) -> Result<(), Error> {
		Ok(())
	}

	fn handle_command(
		&mut self,
		_command: Command,
		_in_data: InData,
		_out_data: OutData,
		_params: &mut Parameters<Params>,
	) -> Result<(), Error> {
		Ok(())
	}
}

#[cfg(test)]
mod preview {
	#[aexlo::preview]
	fn passthrough(fx: &mut aexlo::PluginInstance) {
		fx.render_frame().unwrap();
	}
}
