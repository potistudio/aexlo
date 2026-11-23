//! End-to-end coverage of `RainyDay`, built with the `after-effects` crate.
//! For every command that has no `in_data->sequence_data` (`PARAMS_SETUP`,
//! before the sequence exists) the crate reads it through
//! `PF_EffectSequenceDataSuite1::PF_GetConstSequenceData`.

use std::sync::{Arc, Mutex};

use aexlo::{ObserveLevel, Observer, SuiteCallEvent, UnsupportedEvent};

#[derive(Default)]
struct Record {
	calls: Mutex<Vec<String>>,
	unsupported: Mutex<Vec<String>>,
}

impl Observer for Record {
	fn suite_call(&self, event: &SuiteCallEvent) {
		self.calls
			.lock()
			.unwrap()
			.push(format!("{}::{}", event.suite, event.function));
	}

	fn unsupported(&self, event: &UnsupportedEvent) {
		let gap = format!("{:?} {} {}", event.kind, event.name, event.detail);
		self.unsupported.lock().unwrap().push(gap);
	}
}

/// Loading and rendering reads the sequence data through the suite and asks
/// for nothing aexlo does not serve.
#[test]
fn reads_sequence_data_through_the_suite() {
	let Some(path) = test_e2e::fixture("RainyDay") else {
		eprintln!("skipping: fixture 'RainyDay' not present locally");
		return;
	};
	let record = Arc::new(Record::default());
	let mut instance = aexlo::Host::get()
		.loader(&path)
		.observer(record.clone(), ObserveLevel::Calls)
		.size(64, 48)
		.load()
		.expect("failed to load plugin");
	instance.render_frame().expect("render failed");

	assert!(
		record
			.calls
			.lock()
			.unwrap()
			.iter()
			.any(|c| c.ends_with("PF_GetConstSequenceData")),
		"the crate did not ask for its sequence data"
	);
	assert_eq!(*record.unsupported.lock().unwrap(), Vec::<String>::new());
}
