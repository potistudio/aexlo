//! Loading a plugin: library and entry point resolution, setup commands,
//! dispatching commands, and teardown.

use super::*;

/// Signature of `PluginDataEntryFunction2`: the AE SDK's cross-platform replacement
/// for a binary PiPL resource. Plugins export this under a fixed symbol name and,
/// when called, report their real entry point name (and other PiPL metadata) back
/// through the `PF_PluginDataCB2` callback instead of the host parsing a resource.
type PluginDataEntryFn = unsafe extern "C" fn(
	after_effects_sys::PF_PluginDataPtr,
	after_effects_sys::PF_PluginDataCB2,
	*const after_effects_sys::SPBasicSuite,
	*const std::os::raw::c_char,
	*const std::os::raw::c_char,
) -> after_effects_sys::PF_Err;

#[derive(Default)]
struct PluginDataInfo {
	entry_point_name: Option<String>,
}

unsafe extern "C" fn receive_plugin_data(
	in_ptr: after_effects_sys::PF_PluginDataPtr,
	_in_name: *const after_effects_sys::A_u_char,
	_in_match_name: *const after_effects_sys::A_u_char,
	_in_category: *const after_effects_sys::A_u_char,
	in_entry_point_name: *const after_effects_sys::A_u_char,
	_in_kind: after_effects_sys::A_long,
	_in_api_version_major: after_effects_sys::A_long,
	_in_api_version_minor: after_effects_sys::A_long,
	_in_reserved_info: after_effects_sys::A_long,
	_in_support_url: *const after_effects_sys::A_u_char,
) -> after_effects_sys::A_Err {
	if in_ptr.is_null() || in_entry_point_name.is_null() {
		return PF_Err_INVALID_CALLBACK as after_effects_sys::A_Err;
	}

	let info = unsafe { &mut *(in_ptr as *mut PluginDataInfo) };
	let name = unsafe { CStr::from_ptr(in_entry_point_name as *const std::os::raw::c_char) };
	info.entry_point_name = Some(name.to_string_lossy().into_owned());

	PF_Err_NONE as after_effects_sys::A_Err
}

/// Plugin constructors. Taking a [`Host`] guarantees the process-wide
/// [`AppHost`](crate::AppHost) is fixed before any plugin code runs.
impl Host {
	/// Load a plugin from `path` under this host, then run it through global and params setup.
	///
	/// `path` is the plugin artifact exactly as it exists on disk: a bare
	/// `.aex`/`.dll` file on Windows, or a `.plugin` bundle directory on macOS.
	/// Callers never need to branch on platform -- if `path` is a directory it's
	/// treated as a bundle and the actual binary under `Contents/MacOS/` is
	/// resolved automatically; if it's a file, it's loaded as-is.
	///
	/// # Errors
	/// Returns an error if the binary can't be located or opened, if no entry
	/// point symbol can be resolved, or if the plugin rejects the
	/// `PF_Cmd_GLOBAL_SETUP`, `PF_Cmd_PARAMS_SETUP`, or `PF_Cmd_SEQUENCE_SETUP`
	/// commands.
	pub fn try_load(self, path: impl AsRef<Path>) -> Result<PluginInstance> {
		self.try_load_with(path, crate::Strict::default())
	}

	/// [`Self::try_load`] with strict mode on from the very first command, so
	/// what the plugin allocates during setup is tracked too (see
	/// [`PluginInstance::finish`]).
	///
	/// # Errors
	/// As [`Self::try_load`].
	pub fn try_load_with(self, path: impl AsRef<Path>, strict: crate::Strict) -> Result<PluginInstance> {
		let mut instance = PluginInstance::new(path.as_ref());
		instance.set_strict(strict);

		instance.load()?;
		instance.finalize()?;

		Ok(instance)
	}

	/// Drive an entry point that already lives in this process, skipping the
	/// whole `dlopen` → cdylib → bundle path.
	///
	/// The intended caller is a plugin's own `#[test]`/example: the
	/// `after-effects` `define_effect!` macro exports `EffectMain` as a plain
	/// `#[no_mangle] extern "C"` function, so it can be handed here directly for
	/// a fast, in-process preview that also happens to be debuggable (breakpoints
	/// and real backtraces, unlike a `dlopen`ed cdylib).
	///
	/// Use this when the plugin crate is built against the *same*
	/// `after-effects-sys` version as aexlo, so `EffectMain as PluginEntryPoint`
	/// coerces. Otherwise reach for [`Self::from_entry_raw`].
	///
	/// # Errors
	/// [`AexloError::PluginExecutionFailed`] if the plugin rejects
	/// `PF_Cmd_GLOBAL_SETUP`, `PF_Cmd_PARAMS_SETUP` or `PF_Cmd_SEQUENCE_SETUP`.
	///
	/// # Safety
	/// `entry` must be a valid AE effect entry point that is ABI-compatible with
	/// [`PluginEntryPoint`] and stays callable for the lifetime of the instance.
	pub unsafe fn from_entry(self, entry: PluginEntryPoint) -> Result<PluginInstance> {
		let mut instance = PluginInstance::new(Path::new("<in-process>"));

		instance.entry_point = Some(entry);
		instance.entry_point_name = Some(DEFAULT_ENTRY_POINT_NAME.to_string());
		// No `raw_library`: the entry point is already resident in this process.
		instance.finalize()?;

		Ok(instance)
	}

	/// Like [`Self::from_entry`], but takes the entry point's raw address and
	/// transmutes it to the expected ABI.
	///
	/// This exists for the common case where the plugin crate pulls a *different*
	/// `after-effects-sys` version than aexlo: the two `PluginEntryPoint` types
	/// are then nominally distinct and won't coerce, even though the C ABI is
	/// identical. Passing the address (`EffectMain as usize`) sidesteps the type
	/// mismatch, making ABI compatibility the caller's explicit promise.
	///
	/// # Errors
	/// As [`Self::from_entry`].
	///
	/// # Safety
	/// `entry_addr` must be the address of a function that is ABI-compatible with
	/// [`PluginEntryPoint`] and stays callable for the lifetime of the instance.
	pub unsafe fn from_entry_raw(self, entry_addr: usize) -> Result<PluginInstance> {
		unsafe { self.from_entry_raw_with(entry_addr, crate::Strict::default()) }
	}

	/// [`Self::from_entry_raw`] with strict mode on from the first command.
	///
	/// # Errors
	/// As [`Self::from_entry`].
	///
	/// # Safety
	/// As [`Self::from_entry_raw`].
	pub unsafe fn from_entry_raw_with(self, entry_addr: usize, strict: crate::Strict) -> Result<PluginInstance> {
		let entry: PluginEntryPoint = unsafe { std::mem::transmute(entry_addr) };
		let mut instance = PluginInstance::new(Path::new("<in-process>"));
		instance.set_strict(strict);

		instance.entry_point = Some(entry);
		instance.entry_point_name = Some(DEFAULT_ENTRY_POINT_NAME.to_string());
		instance.finalize()?;

		Ok(instance)
	}
}

impl Drop for PluginInstance {
	/// Tears the plugin down in the same order as After Effects so that the
	/// state it allocated is released.
	///
	/// Failures are only logged, because panicking in `drop` would abort the
	/// process and the plugin is about to be unloaded anyway.
	fn drop(&mut self) {
		self.teardown();
	}
}

impl PluginInstance {
	/// Run the post-load setup shared by every constructor: `GLOBAL_SETUP`,
	/// `PARAMS_SETUP`, then `SEQUENCE_SETUP`. Independent of where the entry point
	/// came from (`dlopen`ed or handed in directly).
	fn finalize(&mut self) -> Result<()> {
		self.setup_global()?;
		self.setup_params()?;
		self.setup_arbitrary_params();
		self.setup_sequence()?;

		Ok(())
	}

	/// Create a new PluginInstance with default values.
	pub(super) fn new(path: &Path) -> Self {
		let interact_callbacks = crate::host::interact::create_interact_callbacks();
		let utility_callbacks = crate::host::utility::create_utility_callbacks();
		let pica = Self::build_pica_suite();

		let input_layer = HostLayer::new(AnyLayer::new(Self::build_layer(
			WIDTH,
			HEIGHT,
			Pixel::<Depth8>::green(),
		)));
		let output_layer = HostLayer::new(AnyLayer::filled(PixelDepthKind::U8, WIDTH, HEIGHT, OPAQUE_BLACK));

		{
			let mut instance_placeholder = PluginInstance {
				raw_library: None,
				entry_point: None,
				entry_point_name: None,
				binary_file_path: path.to_path_buf(),
				resolved_binary_path: None,
				utility_callbacks,
				pica,
				host_info: InDataHost::new(interact_callbacks),
				effect_state: EffectState::default(),
				// Match the output world / default layers so the frame size the plugin
				// sees is consistent across in_data, the checkout rects, and the worlds.
				render_context: {
					let mut ctx = RenderContext::default();
					ctx.set_size(WIDTH as i32, HEIGHT as i32);
					ctx
				},
				out_data: crate::core::helpers::OutDataBuilder::new().build(),
				global_out_flags: 0,
				global_out_flags2: 0,
				params: Vec::new(),
				popup_choices: Vec::new(),
				params_dirty: false,
				params_ptr_cache: Vec::new(),
				input_layer,
				output_layer,
				world: crate::core::helpers::LayerDefBuilder::new()
					.with_size(WIDTH as i32, HEIGHT as i32)
					.build(),
				// Placeholder; wired to `input_layer`'s pixels in `wire_self_pointers`.
				input_world: crate::core::helpers::LayerDefBuilder::new()
					.with_size(WIDTH as i32, HEIGHT as i32)
					.build(),

				smart_render_data: SmartRenderData::new(),
				gpu_context: None,
				gpu_upload_staging: Vec::new(),
				gpu_readback_staging: Vec::new(),
				gpu_input_uploaded: false,
				gpu_data: null_mut(),
				options_button_name: None,
				supported_pixel_formats: Vec::new(),
				mask_paths: Vec::new(),
				linked_layers: HashMap::new(),
				layer_checkouts: HashMap::new(),
				parallel_iterate: true,
				owned_arb_values: Vec::new(),
				observer: None,
				strict: crate::Strict::default(),
				tracker: Default::default(),
				output_guard: None,
				torn_down: false,
			};

			instance_placeholder.wire_self_pointers();
			instance_placeholder.push_input_layer_param();

			instance_placeholder
		}
	}

	/// Build the `SPBasicSuite` vtable handed to the plugin for acquiring host suites.
	fn build_pica_suite() -> Box<after_effects_sys::SPBasicSuite> {
		Box::new(after_effects_sys::SPBasicSuite {
			AcquireSuite: Some(crate::suites::rusty_acquire_suite),
			ReleaseSuite: Some(crate::suites::rusty_release_suite),
			IsEqual: None,
			AllocateBlock: None,
			FreeBlock: None,
			ReallocateBlock: None,
			Undefined: None,
		})
	}

	fn wire_self_pointers(&mut self) {
		self.sync_output_world();
		self.input_world = self.input_layer.as_sys();
	}

	/// Call the plugin with `PF_Cmd_GLOBAL_SETUP` command.
	fn setup_global(&mut self) -> Result<()> {
		self.call_plugin(RawCommand::GlobalSetup, null_mut())?;
		self.global_out_flags = self.out_data.out_flags;
		// The capabilities declared here are what count. `out_data` is shared
		// by every later command, and plugins overwrite its flags in some of
		// them (the SDK samples set `out_flags2` in GPU device setup), so it
		// must not be consulted afterwards.
		self.global_out_flags2 = self.out_data.out_flags2;
		Ok(())
	}

	/// Call the plugin with `PF_Cmd_PARAMS_SETUP` command.
	fn setup_params(&mut self) -> Result<()> {
		self.call_plugin(RawCommand::ParamsSetup, null_mut())
	}

	/// Give every arbitrary-data parameter a value, as After Effects does when
	/// the effect is applied: the plugin allocates it through
	/// `PF_Cmd_ARBITRARY_CALLBACK` (`PF_Arbitrary_NEW_FUNC`, or a
	/// `PF_Arbitrary_COPY_FUNC` of the default). Plugins dereference the value
	/// handle they check out, so leaving it null crashes them.
	fn setup_arbitrary_params(&mut self) {
		for index in 0..self.params.len() {
			if self.params[index].param_type != ParamType::ArbitraryData as PF_ParamType {
				continue;
			}
			// SAFETY: the parameter was verified to be arbitrary data above.
			let arb = unsafe { self.params[index].u.arb_d };
			if !arb.value.is_null() {
				continue;
			}

			let mut value: PF_ArbitraryH = null_mut();
			let mut extra: PF_ArbParamsExtra = unsafe { std::mem::zeroed() };
			extra.which_function = PF_Arbitrary_NEW_FUNC as _;
			extra.id = arb.id;
			extra.u.new_func_params.refconPV = arb.refconPV;
			extra.u.new_func_params.arbPH = &mut value;
			let created = self.call_plugin(RawCommand::ArbitraryCallback, &mut extra as *mut _ as *mut c_void);

			if (created.is_err() || value.is_null()) && !arb.dephault.is_null() {
				let mut extra: PF_ArbParamsExtra = unsafe { std::mem::zeroed() };
				extra.which_function = PF_Arbitrary_COPY_FUNC as _;
				extra.id = arb.id;
				extra.u.copy_func_params.refconPV = arb.refconPV;
				extra.u.copy_func_params.src_arbH = arb.dephault;
				extra.u.copy_func_params.dst_arbPH = &mut value;
				if let Err(err) = self.call_plugin(RawCommand::ArbitraryCallback, &mut extra as *mut _ as *mut c_void) {
					log::warn!("arbitrary param #{index}: copying the default failed: {err:?}");
				}
			}

			if value.is_null() {
				// Legitimate for arbs that only carry a custom UI.
				log::debug!("arbitrary param #{index}: the plugin created no value");
				continue;
			}
			self.params[index].u.arb_d.value = value;
			self.owned_arb_values.push(index);
		}
	}

	/// Hand the arbitrary-data values created by [`Self::setup_arbitrary_params`]
	/// back to the plugin (`PF_Arbitrary_DISPOSE_FUNC`).
	fn dispose_arbitrary_params(&mut self) {
		for index in std::mem::take(&mut self.owned_arb_values) {
			// SAFETY: only arbitrary-data parameters are recorded as owned.
			let arb = unsafe { self.params[index].u.arb_d };
			let mut extra: PF_ArbParamsExtra = unsafe { std::mem::zeroed() };
			extra.which_function = PF_Arbitrary_DISPOSE_FUNC as _;
			extra.id = arb.id;
			extra.u.dispose_func_params.refconPV = arb.refconPV;
			extra.u.dispose_func_params.arbH = arb.value;
			if let Err(err) = self.call_plugin(RawCommand::ArbitraryCallback, &mut extra as *mut _ as *mut c_void) {
				log::warn!("arbitrary param #{index}: disposing the value failed: {err:?}");
			}
			self.params[index].u.arb_d.value = null_mut();
		}
	}

	/// Call the plugin with `PF_Cmd_SEQUENCE_SETUP` command.
	///
	/// This gives the plugin a chance to allocate its per-instance
	/// `sequence_data` (which [`Self::call_plugin`] then propagates back into
	/// `in_data` for subsequent commands). Some effects also perform work here
	/// that later render commands depend on -- e.g. plugins built on the
	/// aescripts licensing library run their license check during
	/// `SEQUENCE_SETUP`/`SEQUENCE_RESETUP`, and render a watermark if it never
	/// runs -- so we always issue it before rendering.
	///
	/// A fresh setup expects a null incoming `sequence_data`; make sure any
	/// stale pointer is cleared so the plugin allocates from scratch rather than
	/// treating garbage as a flattened blob to resurrect.
	fn setup_sequence(&mut self) -> Result<()> {
		self.effect_state.reset_sequence();
		self.out_data.sequence_data = null_mut();
		self.call_plugin(RawCommand::SequenceSetup, null_mut())
	}

	/// Ask the plugin what its real entry point symbol is via the modern
	/// `PluginDataEntryFunction2` protocol -- the same self-description mechanism
	/// After Effects itself uses instead of parsing a binary PiPL resource.
	fn query_declared_entry_point(lib: &Library, pica: &after_effects_sys::SPBasicSuite) -> Option<String> {
		let entry_fn = unsafe { lib.symbol::<PluginDataEntryFn>(PLUGIN_DATA_ENTRY_SYMBOL) }.ok()?;

		let mut info = PluginDataInfo::default();
		let host_name = CString::new(HOST_NAME).ok()?;
		let host_version = CString::new(HOST_VERSION).ok()?;

		let result = unsafe {
			entry_fn(
				&mut info as *mut PluginDataInfo as after_effects_sys::PF_PluginDataPtr,
				Some(receive_plugin_data),
				pica as *const after_effects_sys::SPBasicSuite,
				host_name.as_ptr(),
				host_version.as_ptr(),
			)
		};

		if result != PF_Err_NONE as after_effects_sys::PF_Err {
			log::debug!("{} reported error code {}.", PLUGIN_DATA_ENTRY_SYMBOL, result);
			return None;
		}

		info.entry_point_name
	}

	/// Resolve the plugin's entry point, preferring the name declared via
	/// [`Self::query_declared_entry_point`] and falling back to
	/// [`FALLBACK_ENTRY_POINT_CANDIDATES`] if that's unavailable or unresolvable.
	fn resolve_entry_point(
		lib: &Library,
		pica: &after_effects_sys::SPBasicSuite,
	) -> Result<(PluginEntryPoint, String)> {
		let declared = Self::query_declared_entry_point(lib, pica);
		let candidates = declared
			.as_deref()
			.into_iter()
			.chain(FALLBACK_ENTRY_POINT_CANDIDATES.iter().copied());

		let mut last_error = None;
		for candidate in candidates {
			match unsafe { lib.symbol::<PluginEntryPoint>(candidate) } {
				Ok(symbol) => {
					log::info!("Resolved entry point '{candidate}'.");
					return Ok((symbol, candidate.to_string()));
				}
				Err(err) => {
					log::debug!("Entry point symbol '{}' not resolved: {}", candidate, err);
					last_error = Some(err);
				}
			}
		}

		Err(last_error.expect("FALLBACK_ENTRY_POINT_CANDIDATES is non-empty").into())
	}

	/// Resolve `artifact_path` -- the plugin as it exists on disk -- to the concrete
	/// dynamic library that should be `dlopen`'d.
	///
	/// Callers hand us whatever they'd double-click to install the plugin: a bare
	/// `.aex`/`.dll` file on Windows, or a `.plugin` bundle directory on macOS. Rather
	/// than branching on the compiled/runtime OS (which breaks the moment a flat test
	/// `.dylib` is loaded on macOS, or a bundle is inspected from another host), we
	/// dispatch on the shape of `artifact_path` itself: a directory is a bundle to dig
	/// into, a file is already the binary to load.
	fn resolve_binary_path(artifact_path: &Path) -> Result<PathBuf> {
		if artifact_path.is_dir() {
			return Self::resolve_bundle_binary(artifact_path);
		}

		if artifact_path.is_file() {
			return Ok(artifact_path.to_path_buf());
		}

		Err(AexloError::PluginNotFound {
			path: artifact_path.to_path_buf(),
		})
	}

	/// Find the executable inside a macOS `.plugin` bundle's `Contents/MacOS/`.
	///
	/// AE plugin bundles are required to contain exactly one binary there, so we
	/// use that instead of assuming the binary is named after the bundle.
	fn resolve_bundle_binary(bundle_path: &Path) -> Result<PathBuf> {
		let macos_dir = bundle_path.join("Contents").join("MacOS");

		let mut binaries = std::fs::read_dir(&macos_dir)
			.map_err(|_| AexloError::PluginNotFound {
				path: macos_dir.clone(),
			})?
			.filter_map(|entry| entry.ok())
			.map(|entry| entry.path())
			.filter(|path| path.is_file());

		match (binaries.next(), binaries.next()) {
			(Some(binary), None) => Ok(binary),
			(None, _) => Err(AexloError::PluginNotFound { path: macos_dir }),
			(Some(_), Some(_)) => Err(AexloError::AmbiguousBundle {
				path: bundle_path.to_path_buf(),
			}),
		}
	}

	/// Resolve `self.path` to a binary, `dlopen` it, and resolve its entry point,
	/// storing the results on `self`.
	fn load(&mut self) -> Result<()> {
		let module_path = Self::resolve_binary_path(&self.binary_file_path)?;
		let module_path_str = module_path.display().to_string();

		log::info!("Loading plugin from '{module_path_str}'.");

		let lib = Library::open(&module_path)?;
		let (entry_point, resolved_name) = Self::resolve_entry_point(&lib, self.pica.as_ref())?;

		self.entry_point = Some(entry_point);
		self.entry_point_name = Some(resolved_name.clone());
		self.raw_library = Some(lib);

		// Remembered for the get_platform_data callback (plugin path queries).
		self.resolved_binary_path = Some(module_path.clone());

		log::info!("Resolved entry point symbol: {resolved_name}.");
		log::info!("Loaded plugin '{module_path_str}' successfully.");

		Ok(())
	}

	/// Invoke the resolved entry point with `self.cmd`, updating `self` before and
	/// after the call so the next invocation sees a consistent state.
	///
	/// Before calling: composes a fresh `PF_InData` from the host info, effect
	/// state and render context, with `effect_ref` pointing at `self` (so suite
	/// callbacks can recover the instance via [`Self::get_instance_ptr`]), and
	/// rebuilds the cached param pointer list if `params` was mutated.
	///
	/// After calling: adopts a non-null `out_data.global_data`/`sequence_data`
	/// into the effect state, so plugin-allocated state persists across commands.
	///
	/// `extra_data` is the command-specific extra struct (e.g. `PF_PreRenderExtra`
	/// for `SmartPreRender`), or null for commands that don't take one.
	///
	/// # Errors
	/// Returns [`AexloError::ContainerNotLoaded`] if no entry point has been
	/// resolved yet (see [`Self::load`]), before any other state is touched.
	/// Returns [`AexloError::PluginExecutionFailed`] if the plugin returns a
	/// non-`PF_Err_NONE` code.
	pub(super) fn call_plugin(&mut self, command: RawCommand, extra_data: *mut ::std::os::raw::c_void) -> Result<()> {
		let entry_point = self.entry_point.ok_or(AexloError::PluginNotLoaded)?;
		let effect_ref = self as *mut _ as PF_ProgPtr;

		let entry_point_name = self.entry_point_name.as_deref().unwrap_or(DEFAULT_ENTRY_POINT_NAME);

		// debug, not info: this runs for every render command, so at info level a
		// preview loop drowns the log.
		log::debug!("Executing command: {command:?}");

		if self.params_dirty {
			self.params_ptr_cache = self.params.iter_mut().map(|p| p as *mut _).collect();
			self.params_dirty = false;
		}

		let mut in_data = crate::core::in_data::compose(
			&self.host_info,
			&self.effect_state,
			&self.render_context,
			CallBindings {
				effect_ref,
				utils: self.utility_callbacks.as_mut() as *mut _,
				pica: self.pica.as_mut() as *mut _,
				num_params: self.params.len() as i32,
			},
		);

		let observer = self.observer.clone();
		let started = observer.as_ref().map(|installed| {
			installed.observer.command(&CommandEvent {
				command: command as i32,
				name: crate::observe::command_name(command as i32),
				phase: CommandPhase::Begin,
				duration: None,
				error: None,
			});
			Instant::now()
		});
		let calls = observer
			.as_ref()
			.filter(|installed| installed.level >= ObserveLevel::Calls)
			.map(|installed| installed.observer.clone());
		let calls_guard = crate::observe::enter_calls(calls);
		let name = crate::observe::command_name(command as i32);
		let tracking = self.tracking();
		let resumed = tracking
			.as_ref()
			.and_then(|_| self.tracker.lock().ok().map(|mut t| t.begin(name)));

		let result = crate::suites::iterate::with_parallel_iterate(self.parallel_iterate, || unsafe {
			entry_point(
				command,
				&mut in_data,
				&mut self.out_data,
				self.params_ptr_cache.as_mut_ptr(),
				&mut self.world,
				extra_data,
			)
		});

		if let Some(resumed) = resumed
			&& let Ok(mut tracker) = self.tracker.lock()
		{
			tracker.end(name, resumed);
		}
		drop(tracking);
		drop(calls_guard);
		if let (Some(installed), Some(started)) = (&observer, started) {
			let code = result as i64;
			installed.observer.command(&CommandEvent {
				command: command as i32,
				name: crate::observe::command_name(command as i32),
				phase: CommandPhase::End,
				duration: Some(started.elapsed()),
				error: (code != PF_Err_NONE as i64).then_some(code as i32),
			});
		}

		#[cfg(target_os = "macos")]
		let result = result as u32;

		self.effect_state.absorb(&self.out_data);

		//* ---- Check for errors ---------------------- *//
		#[allow(non_upper_case_globals)]
		match result {
			PF_Err_NONE => {
				log::debug!(
					"Executed command '{command:?}' successfully ({entry_point_name} exited with code {result})."
				);
			}
			code => {
				// The format! only runs on the failure path, never per frame.
				return Err(AexloError::PluginExecutionFailed {
					command: format!("{command:?}"),
					code: code.into(),
				});
			}
		}
		//* -------------------------------------------- *//

		Ok(())
	}

	/// Set the plugin down once, in After Effects' order.
	pub(super) fn teardown(&mut self) {
		// The entry point is missing only when `try_load` failed to resolve it,
		// in which case the plugin was never set up.
		if self.entry_point.is_none() || self.torn_down {
			return;
		}
		self.torn_down = true;

		// Release pre-render data left by an interrupted render while the
		// plugin's code (which owns the delete callback) is still loaded.
		self.end_output_guard();
		self.dispose_pre_render();

		if let Err(err) = self.gpu_device_setdown() {
			log::warn!("PF_Cmd_GPU_DEVICE_SETDOWN failed during drop: {err:?}");
		}

		if let Err(err) = self.call_plugin(RawCommand::SequenceSetdown, null_mut()) {
			log::warn!("PF_Cmd_SEQUENCE_SETDOWN failed during drop: {err:?}");
		}

		self.dispose_arbitrary_params();

		if let Err(err) = self.call_plugin(RawCommand::GlobalSetdown, null_mut()) {
			log::warn!("PF_Cmd_GLOBAL_SETDOWN failed during drop: {err:?}");
		}
		if let Ok(mut tracker) = self.tracker.lock() {
			tracker.torn_down();
		}
	}
}
