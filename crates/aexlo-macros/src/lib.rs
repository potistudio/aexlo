//! Attribute macros for [`aexlo`](https://docs.rs/aexlo): [`macro@preview`]
//! and [`macro@test`]. Both expand to `aexlo-test` paths, so a plugin crate
//! using them adds `aexlo-test` as a dev-dependency.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{Expr, ExprArray, ExprLit, FnArg, Ident, ItemFn, Lit, parse_macro_input};

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
/// test (`cargo test -- --ignored`, or the IDE gutter); it never asserts, it
/// just renders. The generated test is `#[ignore]`d so a plain `cargo test`
/// (CI included) never pays the render cost as a side effect.
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
		#[ignore = "aexlo preview - run via `aexlo dev`/`cargo test -- --ignored`, not plain `cargo test`"]
		fn #name() -> ::aexlo_test::Result {
			// In-process: `EffectMain` is already resident, so hand its address
			// over. By address (not as a typed fn pointer) so a plugin built
			// against a different `after-effects-sys` than aexlo still links.
			::aexlo_test::__private::preview(
				crate::#entry_ident as *const () as usize,
				env!("CARGO_MANIFEST_DIR"),
				module_path!(),
				#name_str,
				|#fixture_pat: &mut ::aexlo_test::aexlo::PluginInstance| #body,
			)
		}
	}
	.into()
}

/// Test an After Effects plugin in-process, once per depth and render path.
///
/// ```ignore
/// #[aexlo::test(depth = [8, 16, 32], render = [smart, gpu], preset = "dot_glow")]
/// fn glow_is_symmetric(fx: &mut aexlo::PluginInstance) -> aexlo_test::Result {
///     fx.set_param_named("Radius", 100.0)?;
///     let frame = aexlo_test::render(fx)?;
///     frame.assert_symmetric_about((320, 240), 0..20, 4.0 / 255.0)?;
///     frame.assert_golden("dot_glow")?;
///     Ok(())
/// }
/// ```
///
/// Expands to one `#[test]` per combination, named after the axes with more
/// than one value (`glow_is_symmetric__d16_gpu`). Each test drives the
/// crate's own `EffectMain` in this process -- debuggers and `dbg!` work --
/// configured from `preset` in the nearest `aexlo.toml` when given. A
/// combination the plugin cannot run prints "skipped" and passes. Unlike
/// [`macro@preview`], the tests are not `#[ignore]`d.
///
/// Arguments: `depth` (8, 16, 32 or a list), `render` (`auto`, `legacy`,
/// `smart`, `gpu` or a list), `preset = "name"`, `entry = "EffectMain"`.
/// The body may return `()` or `aexlo_test::Result`; `aexlo_test`'s
/// extension traits (`set_param_named`, the frame assertions) are in scope.
#[proc_macro_attribute]
pub fn test(attr: TokenStream, item: TokenStream) -> TokenStream {
	let mut entry = String::from("EffectMain");
	let mut depths: Vec<u32> = Vec::new();
	let mut renders: Vec<String> = Vec::new();
	let mut preset: Option<String> = None;
	if !attr.is_empty() {
		let parser = syn::meta::parser(|meta| {
			if meta.path.is_ident("entry") {
				entry = meta.value()?.parse::<syn::LitStr>()?.value();
			} else if meta.path.is_ident("preset") {
				preset = Some(meta.value()?.parse::<syn::LitStr>()?.value());
			} else if meta.path.is_ident("depth") {
				for expr in list(meta.value()?.parse::<Expr>()?) {
					match expr {
						Expr::Lit(ExprLit { lit: Lit::Int(int), .. }) => {
							let bits: u32 = int.base10_parse()?;
							if ![8, 16, 32].contains(&bits) {
								return Err(syn::Error::new(int.span(), "depth must be 8, 16 or 32"));
							}
							depths.push(bits);
						}
						other => return Err(syn::Error::new_spanned(other, "expected a depth: 8, 16 or 32")),
					}
				}
			} else if meta.path.is_ident("render") {
				for expr in list(meta.value()?.parse::<Expr>()?) {
					let name = match &expr {
						Expr::Path(path) if path.path.get_ident().is_some() => {
							path.path.get_ident().map(Ident::to_string).unwrap_or_default()
						}
						Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) => s.value(),
						other => {
							return Err(syn::Error::new_spanned(
								other,
								"expected a render path: auto, legacy, smart or gpu",
							));
						}
					};
					if !["auto", "legacy", "smart", "gpu"].contains(&name.as_str()) {
						return Err(syn::Error::new_spanned(
							expr,
							"expected a render path: auto, legacy, smart or gpu",
						));
					}
					renders.push(name);
				}
			} else {
				return Err(
					meta.error("unsupported `aexlo::test` argument (expected `depth`, `render`, `preset` or `entry`)")
				);
			}
			Ok(())
		});
		parse_macro_input!(attr with parser);
	}
	let entry_ident = format_ident!("{}", entry);

	let mut func = parse_macro_input!(item as ItemFn);
	if !matches!(func.sig.inputs.first(), Some(FnArg::Typed(_))) || func.sig.inputs.len() != 1 {
		return syn::Error::new_spanned(
			&func.sig,
			"`#[aexlo::test]` functions take one fixture, e.g. `fn t(fx: &mut aexlo::PluginInstance)`",
		)
		.to_compile_error()
		.into();
	}
	let name = func.sig.ident.clone();
	let inner = format_ident!("__aexlo_test_{}", name);
	func.sig.ident = inner.clone();
	// The extension traits, in scope for the body.
	let body = &func.block;
	func.block = syn::parse_quote!({
		#[allow(unused_imports)]
		use ::aexlo_test::prelude::*;
		#body
	});

	let depth_axis: Vec<Option<u32>> = if depths.is_empty() {
		vec![None]
	} else {
		depths.iter().copied().map(Some).collect()
	};
	let render_axis: Vec<Option<String>> = if renders.is_empty() {
		vec![None]
	} else {
		renders.iter().cloned().map(Some).collect()
	};
	let preset = match &preset {
		Some(p) => quote!(::core::option::Option::Some(#p)),
		None => quote!(::core::option::Option::None),
	};

	let mut tests = Vec::new();
	for depth in &depth_axis {
		for render in &render_axis {
			// Name and golden suffix carry only the axes that vary (§4.8).
			let mut name_parts = Vec::new();
			let mut golden = String::new();
			if let (Some(d), true) = (depth, depth_axis.len() > 1) {
				name_parts.push(format!("d{d}"));
				golden.push_str(&format!(".d{d}"));
			}
			if let (Some(r), true) = (render, render_axis.len() > 1) {
				name_parts.push(r.clone());
			}
			let test_name = if name_parts.is_empty() {
				name.clone()
			} else {
				Ident::new(&format!("{name}__{}", name_parts.join("_")), Span::call_site())
			};
			let test_str = test_name.to_string();
			let depth = match depth {
				Some(d) => quote!(::core::option::Option::Some(#d)),
				None => quote!(::core::option::Option::None),
			};
			let render = match render {
				Some(r) => quote!(::core::option::Option::Some(#r)),
				None => quote!(::core::option::Option::None),
			};
			tests.push(quote! {
				#[test]
				#[allow(non_snake_case)]
				fn #test_name() {
					::aexlo_test::__private::run_test(
						::aexlo_test::__private::TestSpec {
							entry: crate::#entry_ident as *const () as usize,
							crate_dir: env!("CARGO_MANIFEST_DIR"),
							name: #test_str,
							preset: #preset,
							depth: #depth,
							render: #render,
							golden_suffix: #golden,
						},
						#inner,
					)
				}
			});
		}
	}

	quote! {
		#[allow(non_snake_case, dead_code)]
		#func
		#(#tests)*
	}
	.into()
}

/// The elements of `[a, b]`, or `a` alone.
fn list(expr: Expr) -> Vec<Expr> {
	match expr {
		Expr::Array(ExprArray { elems, .. }) => elems.into_iter().collect(),
		other => vec![other],
	}
}
