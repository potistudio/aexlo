//! Diagnostics for logging function calls and their arguments/results.
//!
//! This module provides structured diagnostic output for debugging plugin callbacks.
//! The design separates data structure from presentation for flexibility.

use std::borrow::Cow;

/// Colorize a value string based on its detected type.
#[cfg(feature = "diagnostics")]
fn colorize_value(value: &str) -> colored::ColoredString {
	use colored::Colorize;

	// None
	if value == "None" || value == "null" {
		return value.red();
	}

	// Some(...)
	if value.starts_with("Some(") {
		return value.magenta();
	}

	// Pointer address: 0x...
	if value.starts_with("0x") || value.starts_with("0X") {
		return value.cyan();
	}

	// Boolean
	if value == "true" || value == "false" {
		return value.magenta().bold();
	}

	// Number (integer or float)
	if value.parse::<i64>().is_ok() || value.parse::<f64>().is_ok() {
		return value.blue();
	}

	// String literal (quoted)
	if (value.starts_with('"') && value.ends_with('"'))
		|| (value.starts_with('\'') && value.ends_with('\''))
	{
		return value.green();
	}

	// Default: yellow
	value.yellow()
}

/// A structured diagnostic record.
///
/// This is the pure data structure, separate from any presentation concerns.
#[derive(Debug, Clone)]
pub struct Diagnostic<'a> {
	/// Name of the function/callback being diagnosed.
	pub name: Cow<'a, str>,
	/// Arguments passed to the function.
	pub args: Vec<(Cow<'a, str>, String)>,
	/// Result of the function call, if any.
	pub result: Option<String>,
}

impl<'a> Diagnostic<'a> {
	/// Print the diagnostic with colored output to stdout.
	#[cfg(feature = "diagnostics")]
	pub fn print_colored(&self) {
		use colored::Colorize;

		let timestamp = chrono::Utc::now().format("%H:%M:%S%.6f").to_string();
		let level = "<DEBUG>".green().bold();
		let message = "function has called".white().bold();

		println!("[{timestamp}] {level} {message}");
		println!("  ╭─[ {} ]", self.name);

		for (arg_name, arg_value) in &self.args {
			println!("  │   {}: {}", arg_name, colorize_value(arg_value));
		}

		println!("{}", "  ◇".blue());

		if let Some(ref result) = self.result {
			println!("  ╰─► {}", colorize_value(result));
		}

		println!();
	}

	/// Emit via the `log` crate (no colors, structured).
	#[cfg(feature = "diagnostics")]
	pub fn log(&self) {
		let args_str: Vec<String> = self
			.args
			.iter()
			.map(|(k, v)| format!("{}={}", k, v))
			.collect();

		if let Some(ref result) = self.result {
			log::debug!("{}({}) -> {}", self.name, args_str.join(", "), result);
		} else {
			log::debug!("{}({})", self.name, args_str.join(", "));
		}
	}
}

/// Builder for creating [`Diagnostic`] records.
///
/// Uses `Cow<str>` for names to avoid heap allocation on static strings.
pub struct DiagnosticBuilder<'a> {
	name: Cow<'a, str>,
	args: Vec<(Cow<'a, str>, String)>,
	result: Option<String>,
}

impl<'a> DiagnosticBuilder<'a> {
	/// Create a new DiagnosticBuilder instance.
	pub fn new() -> Self {
		Self {
			name: Cow::Borrowed(""),
			args: Vec::new(),
			result: None,
		}
	}

	/// Set the name of the function being diagnosed.
	pub fn set_name(&mut self, name: impl Into<Cow<'a, str>>) -> &mut Self {
		self.name = name.into();
		self
	}

	/// Add an argument to the diagnostic.
	pub fn add_arg(&mut self, name: impl Into<Cow<'a, str>>, value: impl ToString) -> &mut Self {
		self.args.push((name.into(), value.to_string()));
		self
	}

	/// Set the result of the function being diagnosed.
	pub fn set_result(&mut self, result: impl ToString) -> &mut Self {
		self.result = Some(result.to_string());
		self
	}

	/// Build the diagnostic record.
	pub fn build(&self) -> Diagnostic<'a> {
		Diagnostic {
			name: self.name.clone(),
			args: self.args.clone(),
			result: self.result.clone(),
		}
	}

	/// Build and emit the diagnostic (convenience method).
	#[cfg(feature = "diagnostics")]
	pub fn emit(&self) {
		self.build().print_colored();
	}

	/// No-op when diagnostics feature is disabled.
	#[cfg(not(feature = "diagnostics"))]
	#[inline(always)]
	pub fn emit(&self) {}
}

impl Default for DiagnosticBuilder<'_> {
	fn default() -> Self {
		Self::new()
	}
}
