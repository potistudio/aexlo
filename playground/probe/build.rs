//! Generates and embeds the PiPL resource via the `pipl` crate - the same
//! machinery the `after-effects` crate's own examples use. On Windows it
//! compiles a `16000 PiPL` resource into the DLL (which real After Effects
//! requires to recognize a .aex); on macOS it drops a `.rsrc` next to the
//! target dir.
//!
//! `playground harness pipl` parses the embedded resource back out of the
//! built DLL, so a regression here is caught before AE ever sees it.
//!
//! Keep the values in sync with `src/lib.rs`: AE cross-checks the PiPL
//! against what GLOBAL_SETUP writes into out_data and complains on mismatch.
//!
//! The `smart` feature selects the SmartRender variant: another effect with
//! its own name and match name, so both variants can be installed in After
//! Effects side by side.

use pipl::*;

fn main() {
	println!("cargo:rerun-if-changed=build.rs");

	let smart = std::env::var_os("CARGO_FEATURE_SMART").is_some();

	// Mirrors EFFECT_NAME / MATCH_NAME / OUT_FLAGS / OUT_FLAGS2 in lib.rs.
	let (name, match_name, out_flags, out_flags2) = if smart {
		(
			"Aexlo Probe Smart",
			"AEXLO Probe Smart",
			OutFlags::DeepColorAware,
			OutFlags2::SupportsThreadedRendering
				| OutFlags2::SupportsSmartRender
				| OutFlags2::FloatColorAware
				| OutFlags2::SupportsGpuRenderF32,
		)
	} else {
		(
			"Aexlo Probe",
			"AEXLO Probe",
			OutFlags::empty(),
			OutFlags2::SupportsThreadedRendering,
		)
	};

	plugin_build(vec![
		Property::Kind(PIPLType::AEEffect),
		Property::Name(name),
		Property::Category("aexlo"),
		Property::CodeWin64X86("EffectMain"),
		Property::CodeMacIntel64("EffectMain"),
		Property::CodeMacARM64("EffectMain"),
		Property::AE_PiPL_Version { major: 2, minor: 0 },
		// PF_PLUG_IN_VERSION / PF_PLUG_IN_SUBVERS from after-effects-sys 0.4.0.
		Property::AE_Effect_Spec_Version { major: 13, minor: 29 },
		// Mirrors PROBE_PF_VERSION in lib.rs: PF_VERSION(1, 0, 0, RELEASE, 1).
		Property::AE_Effect_Version {
			version: 1,
			subversion: 0,
			bugversion: 0,
			stage: Stage::Release,
			build: 1,
		},
		Property::AE_Effect_Info_Flags(0),
		Property::AE_Effect_Global_OutFlags(out_flags),
		Property::AE_Effect_Global_OutFlags_2(out_flags2),
		Property::AE_Effect_Match_Name(match_name),
		Property::AE_Reserved_Info(0),
		Property::AE_Effect_Support_URL("https://github.com/potistudio/aexlo"),
	]);
}
