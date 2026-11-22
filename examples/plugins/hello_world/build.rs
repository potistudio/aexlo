use pipl::*;

fn main() {
	pipl::plugin_build(vec![
		Property::Kind(PIPLType::AEEffect),
		Property::Name("aexlo demo - Hello World"),
		Property::Category("POTI"),
		Property::AE_Effect_Version {
			version: 1,
			subversion: 0,
			bugversion: 0,
			stage: Stage::Release,
			build: 1,
		},
		Property::AE_PiPL_Version { minor: 2, major: 0 },
		Property::AE_Effect_Spec_Version { minor: 13, major: 28 },
		Property::AE_Reserved_Info(0),
		Property::AE_Effect_Global_OutFlags(OutFlags::None),
		Property::AE_Effect_Global_OutFlags_2(OutFlags2::SupportsThreadedRendering),
		Property::AE_Effect_Support_URL("https://github.com/potistudio"),
		Property::AE_Effect_Match_Name("POTI AEXLO DEMO HELLO WORLD"),
	]);
}
