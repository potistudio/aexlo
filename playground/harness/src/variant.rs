//! The two probe builds: the legacy effect (`PF_Cmd_RENDER`) and the smart
//! effect (`PF_Cmd_SMART_PRE_RENDER` / `PF_Cmd_SMART_RENDER`, deep and float
//! color, GPU device setup). Out flags must match the PiPL, so each variant
//! is its own build of `aexlo-probe`; the smart one is built with the `smart`
//! feature into its own target dir so the two never overwrite each other.

use std::path::{Path, PathBuf};

use aexlo::{Depth8, Depth16, Depth32, Layer, PixelDepthKind, PluginInstance};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
	Legacy,
	Smart,
}

impl Variant {
	pub const ALL: [Variant; 2] = [Variant::Legacy, Variant::Smart];

	/// `--variant legacy|smart`; legacy when absent.
	pub fn from_args(args: &[String]) -> anyhow::Result<Self> {
		let Some(index) = args.iter().position(|a| a == "--variant") else {
			return Ok(Variant::Legacy);
		};
		match args.get(index + 1).map(String::as_str) {
			Some("legacy") => Ok(Variant::Legacy),
			Some("smart") => Ok(Variant::Smart),
			other => anyhow::bail!("--variant takes legacy or smart, got {other:?}"),
		}
	}

	/// The installed file name in After Effects.
	pub fn aex_name(self) -> &'static str {
		match self {
			Variant::Legacy => "AexloProbe.aex",
			Variant::Smart => "AexloProbeSmart.aex",
		}
	}

	/// The effect's name in the Effect menu.
	pub fn effect_name(self) -> &'static str {
		match self {
			Variant::Legacy => "Aexlo Probe",
			Variant::Smart => "Aexlo Probe Smart",
		}
	}

	/// Default trace file under `target/probe/`.
	pub fn trace_file(self) -> &'static str {
		match self {
			Variant::Legacy => "trace-aexlo.jsonl",
			Variant::Smart => "trace-aexlo-smart.jsonl",
		}
	}

	/// Default preview file under `target/probe/`.
	pub fn preview_file(self) -> &'static str {
		match self {
			Variant::Legacy => "preview-aexlo.png",
			Variant::Smart => "preview-aexlo-smart.png",
		}
	}

	/// The cargo target dir this variant builds into.
	pub fn target_dir(self, root: &Path) -> PathBuf {
		let base = cargo_target_dir(root);
		match self {
			Variant::Legacy => base,
			Variant::Smart => base.join("probe-smart"),
		}
	}

	/// Extra `cargo build` arguments for this variant.
	pub fn cargo_args(self, root: &Path) -> Vec<String> {
		match self {
			Variant::Legacy => Vec::new(),
			Variant::Smart => vec![
				"--features".into(),
				"smart".into(),
				"--target-dir".into(),
				self.target_dir(root).display().to_string(),
			],
		}
	}

	/// Render the frames this variant is measured on. Legacy: one frame at
	/// the input's 8 bpc. Smart: one frame at 8, 16 and 32 bpc, so every
	/// `smart.<bpc>.*` fact appears in one trace. The smart build must declare
	/// all three depths; a missing one is a broken build, not a host
	/// difference, so it fails here instead of leaving a gap for `diff`.
	pub fn render(self, instance: &mut PluginInstance, input: &Layer<Depth8>) -> anyhow::Result<()> {
		if self == Variant::Legacy {
			instance.render_frame()?;
			return Ok(());
		}

		for depth in [PixelDepthKind::U8, PixelDepthKind::U16, PixelDepthKind::F32] {
			anyhow::ensure!(
				instance.supports_depth(depth),
				"the smart probe does not declare {depth:?}; check OUT_FLAGS / OUT_FLAGS2 in probe/src/lib.rs 				 and the PiPL in probe/build.rs"
			);
			match depth {
				PixelDepthKind::U8 => instance.set_input_layer(input.clone()),
				PixelDepthKind::U16 => instance.set_input_layer(input.convert::<Depth16>()),
				PixelDepthKind::F32 => instance.set_input_layer(input.convert::<Depth32>()),
			}
			instance.render_frame()?;
		}
		Ok(())
	}
}

/// `CARGO_TARGET_DIR` when set (as cargo itself resolves it), else
/// `<workspace>/target`.
fn cargo_target_dir(root: &Path) -> PathBuf {
	match std::env::var_os("CARGO_TARGET_DIR") {
		Some(dir) => root.join(dir),
		None => root.join("target"),
	}
}
