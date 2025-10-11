use colored::Colorize;

/// Diagnostic builder for logging function calls and their arguments/results
pub struct DiagnosticBuilder {
	name: String,
	// message: String,
	args: Vec<(String, String)>,
	result: Option<String>,
}

impl DiagnosticBuilder {
	/// Create a new DiagnosticBuilder instance
	pub fn new() -> Self {
		Self {
			name: String::new(),
			// message: String::new(),
			args: Vec::new(),
			result: None,
		}
	}

	/// Add an argument to the diagnostic
	pub fn add_arg(&mut self, name: impl Into<String>, value: impl ToString) -> &mut Self {
		self.args.push((name.into(), value.to_string()));
		self
	}

	/// Set the name of the function being diagnosed
	pub fn set_name(&mut self, name: impl Into<String>) -> &mut Self {
		self.name = name.into();
		self
	}

	/// Set the result of the function being diagnosed
	pub fn set_result(&mut self, result: impl ToString) -> &mut Self {
		self.result = Some(result.to_string());
		self
	}

	/// Emit the diagnostic log
	#[cfg(feature = "diagnostics")]
	pub fn emit(&mut self) {
		let timestamp = chrono::Utc::now().format("%H:%M:%S%.6f").to_string();
		let level = "<DEBUG>".green().bold();
		let message = "function has called".white().bold();

		let DiagnosticBuilder {
			name, args, result, ..
		} = self;

		println!("[{timestamp}] {level} {message}");
		println!("  ╭─[ {} ]", name);

		for (arg_name, arg_value) in args {
			println!("  │   {}: {}", arg_name, arg_value.yellow());
		}

		println!("{}", "  ◇".blue());

		if let Some(x) = result {
			println!("  ╰─► {}", x.yellow());
		}

		println!();
	}

	#[cfg(not(feature = "diagnostics"))]
	#[inline(always)]
	pub fn emit(&self) {}
}

impl Default for DiagnosticBuilder {
	fn default() -> Self {
		Self::new()
	}
}
