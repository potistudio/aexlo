//! Attribute macros for [`aexlo`](https://docs.rs/aexlo).
//!
//! Currently just [`macro@preview`], which turns a plain function into an
//! in-process visual preview of an After Effects plugin.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, ItemFn, parse_macro_input};

/// Preview an After Effects plugin in-process -- no cdylib, bundle, or `dlopen`.
///
/// The macro hides only the awkward parts: loading the in-process `EffectMain`,
/// the `#[test]` wrapper, and saving/opening the result. Your function body does
/// the actual driving -- **including the render** -- so what happens is visible
/// in the code rather than implied by an empty body:
///
/// ```ignore
/// #[aexlo::preview]
/// fn with_blur(fx: &mut aexlo::PluginInstance) {
///     fx.set_param(1, aexlo::ParamValue::Float(0.8)).unwrap();
///     fx.render_frame().unwrap();
/// }
/// ```
///
/// After the body runs, the macro writes a full-quality PNG and -- when
/// `AEXLO_PREVIEW` is set -- opens it in the OS image viewer. Run it like any
/// test (`cargo test`, or the IDE gutter); it never asserts, it just renders.
///
/// The function must take a fixture parameter (a `&mut aexlo::PluginInstance`).
/// If the plugin's entry point isn't named `EffectMain`, override it with
/// `#[aexlo::preview(entry = "EntryPointFunc")]`.
#[proc_macro_attribute]
pub fn preview(attr: TokenStream, item: TokenStream) -> TokenStream {
	// Entry-point symbol to drive; defaults to the `after-effects` crate's export.
	let mut entry = String::from("EffectMain");
	if !attr.is_empty() {
		let parser = syn::meta::parser(|meta| {
			if meta.path.is_ident("entry") {
				entry = meta.value()?.parse::<syn::LitStr>()?.value();
				Ok(())
			} else {
				Err(meta.error("unsupported `aexlo::preview` argument (expected `entry = \"...\"`)"))
			}
		});
		parse_macro_input!(attr with parser);
	}
	let entry_ident = format_ident!("{}", entry);

	let func = parse_macro_input!(item as ItemFn);
	let name = &func.sig.ident;
	let name_str = name.to_string();
	let body = &func.block;

	// The fixture is mandatory: the body drives (and renders) the plugin through
	// it, so there is nothing to preview without it.
	let fixture_pat = match func.sig.inputs.first() {
		Some(FnArg::Typed(pat_type)) => pat_type.pat.clone(),
		_ => {
			return syn::Error::new_spanned(
				&func.sig,
				"`#[aexlo::preview]` functions must take a fixture, \
				 e.g. `fn preview(fx: &mut aexlo::PluginInstance)`",
			)
			.to_compile_error()
			.into();
		}
	};

	quote! {
		#[test]
		fn #name() -> ::aexlo::Result<()> {
			// In-process: `EffectMain` is already resident, so hand its address
			// to aexlo. `from_entry_raw` (not `from_entry`) so a plugin built
			// against a different `after-effects-sys` than aexlo still links.
			let mut __aexlo_fx = unsafe {
				::aexlo::PluginInstance::from_entry_raw(crate::#entry_ident as *const () as usize)
			}?;

			{
				let #fixture_pat = &mut __aexlo_fx;
				#body
			}

			let __path = ::aexlo::preview_path(
				env!("CARGO_MANIFEST_DIR"),
				module_path!(),
				#name_str,
			);
			__aexlo_fx.save_preview(&__path)?;
			eprintln!("aexlo::preview: wrote {}", __path.display());

			// AEXLO_PREVIEW: unset = save only, `live` = keep an `aexlo view`
			// window updated (pair with a re-runner like `bacon`), else open once.
			match ::aexlo::preview_mode() {
				::aexlo::PreviewMode::Off => {}
				::aexlo::PreviewMode::Once => ::aexlo::open_in_viewer(&__path)?,
				::aexlo::PreviewMode::Live => ::aexlo::ensure_live_viewer(&__path)?,
			}

			Ok(())
		}
	}
	.into()
}
