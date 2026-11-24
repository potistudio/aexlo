//! Tests that need something this machine may not have.
//!
//! libtest cannot skip a test while it runs, and one that returns early is
//! counted as passed. So `needs_fixture` decides when the test is compiled
//! and reports it as ignored, with the reason. `cargo test -- --ignored`
//! still runs it, and the test then fails for want of the fixture.

use proc_macro::{Delimiter, TokenStream, TokenTree};

/// Ignore the test unless this machine has what it needs. Write it above
/// `#[test]`.
///
/// - `#[needs_fixture("A")]`: the fixture `A` in `fixtures/plugins`.
/// - `#[needs_fixture("A", "B")]`: any one of them.
/// - `#[needs_fixture]`: any fixture at all.
/// - `#[needs_fixture("A", gpu)]`: also a GPU that aexlo can render on.
#[proc_macro_attribute]
pub fn needs_fixture(args: TokenStream, item: TokenStream) -> TokenStream {
	let mut wanted = Vec::new();
	let mut gpu = false;
	for token in args {
		match token.to_string().as_str() {
			"," => {}
			"gpu" => gpu = true,
			text => match text.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
				Some(name) => wanted.push(name.to_string()),
				None => return error("expected fixture names in quotes, or `gpu`"),
			},
		}
	}
	// `#[test]` reads `#[ignore]` when it expands, so it must still be ahead.
	if !has_test_attribute(item.clone()) {
		return error("write `#[needs_fixture]` above `#[test]`");
	}

	let present: Vec<&str> = env!("FIXTURE_GATE_PRESENT")
		.split('|')
		.filter(|name| !name.is_empty())
		.collect();
	let found = if wanted.is_empty() {
		!present.is_empty()
	} else {
		wanted
			.iter()
			.any(|name| present.iter().any(|have| have.eq_ignore_ascii_case(name)))
	};
	let reason = if !found && wanted.is_empty() {
		"needs a plugin in fixtures/plugins".to_string()
	} else if !found {
		format!("needs fixture {}", wanted.join(" or "))
	} else if gpu && env!("FIXTURE_GATE_GPU") != "true" {
		"needs a GPU (CUDA or Metal)".to_string()
	} else {
		return item;
	};

	let mut out: TokenStream = format!("#[ignore = {reason:?}]").parse().expect("an attribute");
	out.extend(item);
	out
}

fn has_test_attribute(item: TokenStream) -> bool {
	item.into_iter().any(|token| match token {
		TokenTree::Group(group) if group.delimiter() == Delimiter::Bracket => group.stream().to_string() == "test",
		_ => false,
	})
}

fn error(message: &str) -> TokenStream {
	format!("compile_error!({message:?});").parse().expect("an item")
}
