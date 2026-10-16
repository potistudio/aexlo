//! Errors, sorted by who is at fault: that decides the exit code (§12).

/// Who is at fault for an [`Error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ErrorKind {
	/// The plugin ran and was judged bad: it returned a `PF_Err`, or could not
	/// be loaded. A run ending this way is an `error` outcome.
	Plugin,
	/// The manifest or the command line is invalid (exit code 2).
	Invalid,
	/// The harness could not reach a verdict (exit code 3): a worker that
	/// cannot be spawned, an unwritable directory, an unreadable golden, ...
	Harness,
}

/// An error from the harness, carrying who is at fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
	pub kind: ErrorKind,
	pub message: String,
}

impl Error {
	pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
		Self {
			kind,
			message: message.into(),
		}
	}

	pub fn invalid(message: impl Into<String>) -> Self {
		Self::new(ErrorKind::Invalid, message)
	}

	pub fn harness(message: impl Into<String>) -> Self {
		Self::new(ErrorKind::Harness, message)
	}

	pub fn plugin(message: impl Into<String>) -> Self {
		Self::new(ErrorKind::Plugin, message)
	}

	/// Prefix the message with what was being done.
	pub fn context(mut self, context: impl std::fmt::Display) -> Self {
		self.message = format!("{context}: {}", self.message);
		self
	}

	/// The process exit code this error maps to on its own.
	pub fn exit_code(&self) -> u8 {
		match self.kind {
			ErrorKind::Plugin => 1,
			ErrorKind::Invalid => 2,
			ErrorKind::Harness => 3,
		}
	}
}

impl std::fmt::Display for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.message)
	}
}

impl std::error::Error for Error {}

impl From<aexlo::AexloError> for Error {
	fn from(err: aexlo::AexloError) -> Self {
		Self::plugin(err.to_string())
	}
}

pub type Result<T> = std::result::Result<T, Error>;
