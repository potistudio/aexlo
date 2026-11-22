//! `--color`, shared by the commands whose text output is colored.

use anstream::ColorChoice;

/// `--color`: when text output is colored.
#[derive(clap::ValueEnum, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ColorWhen {
	#[default]
	Auto,
	Always,
	Never,
}

impl ColorWhen {
	/// Make this the choice for every stream the process writes.
	pub fn apply(self) {
		match self {
			ColorWhen::Auto => {}
			ColorWhen::Always => ColorChoice::Always.write_global(),
			ColorWhen::Never => ColorChoice::Never.write_global(),
		}
	}
}

/// Whether text written to stdout should be colored, after [`ColorWhen::apply`].
pub fn stdout() -> bool {
	anstream::AutoStream::choice(&std::io::stdout()) != ColorChoice::Never
}
