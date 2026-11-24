//! Read-only project information for the synthetic one-layer host.
use super::layer::{Timeline, current_effect};
use crate::sys::*;

unsafe extern "C" fn count(out: *mut A_long) -> A_Err {
	crate::core::ffi::guard("project_count", || {
		let Some(out) = (unsafe { out.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if Timeline::of_effect(current_effect().cast()).is_none() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = 1;
		PF_Err_NONE
	})
}
unsafe extern "C" fn by_index(index: A_long, out: *mut AEGP_ProjectH) -> A_Err {
	crate::core::ffi::guard("project_by_index", || {
		let Some(out) = (unsafe { out.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		*out = std::ptr::null_mut();
		if index != 0 {
			return PF_Err_INVALID_INDEX;
		}
		let effect = current_effect();
		if Timeline::of_effect(effect.cast()).is_none() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		*out = effect.cast();
		PF_Err_NONE
	})
}
unsafe extern "C" fn name(project: AEGP_ProjectH, out: *mut A_char) -> A_Err {
	crate::core::ffi::guard("project_name", || {
		if out.is_null() || Timeline::of_effect(project.cast()).is_none() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		unsafe { std::ptr::copy_nonoverlapping(c"aexlo".as_ptr(), out, 6) };
		PF_Err_NONE
	})
}
unsafe extern "C" fn dirty(project: AEGP_ProjectH, out: *mut A_Boolean) -> A_Err {
	crate::core::ffi::guard("project_dirty", || {
		super::layer::answer_comp(project.cast(), out, |_| 0)
	})
}
unsafe extern "C" fn depth(project: AEGP_ProjectH, out: *mut AEGP_ProjBitDepth) -> A_Err {
	crate::core::ffi::guard("project_depth", || {
		if Timeline::of_effect(project.cast()).is_none() || out.is_null() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		let instance = unsafe {
			crate::PluginInstance::get_instance_ptr(project.cast())
				.unwrap()
				.as_ref()
		};
		let value = match instance.input_depth() {
			crate::PixelDepthKind::U8 => AEGP_ProjBitDepth_8,
			crate::PixelDepthKind::U16 => AEGP_ProjBitDepth_16,
			crate::PixelDepthKind::F32 => AEGP_ProjBitDepth_32,
		};
		unsafe {
			*out = value as AEGP_ProjBitDepth;
		}
		PF_Err_NONE
	})
}
macro_rules! unsupported {
	($name:ident ($($arg:ident : $ty:ty),*)) => {
		unsafe extern "C" fn $name($($arg: $ty),*) -> A_Err {
			crate::core::ffi::guard(stringify!($name), || { $(let _ = $arg;)* log::warn!("Project Suite: {} is unsupported", stringify!($name)); A_Err_GENERIC })
		}
	};
}
unsafe extern "C" fn path(project: AEGP_ProjectH, out: *mut AEGP_MemHandle) -> A_Err {
	crate::core::ffi::guard("project_path", || {
		let Some(out) = (unsafe { out.as_mut() }) else {
			return PF_Err_BAD_CALLBACK_PARAM;
		};
		if Timeline::of_effect(project.cast()).is_none() {
			return PF_Err_BAD_CALLBACK_PARAM;
		}
		// The synthetic project has never been saved: AE returns an empty UTF-16 string.
		*out = super::aegp_memory::handle_from_bytes(vec![0, 0]);
		PF_Err_NONE
	})
}
unsupported!(root(_project: AEGP_ProjectH, _out: *mut AEGP_ItemH));
unsupported!(save(_project: AEGP_ProjectH, _path: *const A_UTF16Char));
unsupported!(time_display(_project: AEGP_ProjectH, _out: *mut AEGP_TimeDisplay2));
unsupported!(set_time_display(_project: AEGP_ProjectH, _value: *const AEGP_TimeDisplay2));
unsupported!(new_project(_out: *mut AEGP_ProjectH));
unsupported!(open(_path: *const A_UTF16Char, _out: *mut AEGP_ProjectH));
unsupported!(set_depth(_project: AEGP_ProjectH, _depth: AEGP_ProjBitDepth));
pub(crate) static SUITE: AEGP_ProjSuite5 = AEGP_ProjSuite5 {
	AEGP_GetNumProjects: Some(count),
	AEGP_GetProjectByIndex: Some(by_index),
	AEGP_GetProjectName: Some(name),
	AEGP_GetProjectPath: Some(path),
	AEGP_GetProjectRootFolder: Some(root),
	AEGP_SaveProjectToPath: Some(save),
	AEGP_GetProjectTimeDisplay: Some(time_display),
	AEGP_SetProjectTimeDisplay: Some(set_time_display),
	AEGP_ProjectIsDirty: Some(dirty),
	AEGP_SaveProjectAs: Some(save),
	AEGP_NewProject: Some(new_project),
	AEGP_OpenProjectFromPath: Some(open),
	AEGP_GetProjectBitDepth: Some(depth),
	AEGP_SetProjectBitDepth: Some(set_depth),
};

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicBool, Ordering};
	static OK: AtomicBool = AtomicBool::new(false);
	unsafe extern "C" fn entry(
		cmd: PF_Cmd,
		_: *mut PF_InData,
		_: *mut PF_OutData,
		_: PF_ParamList,
		_: *mut PF_LayerDef,
		_: *mut std::ffi::c_void,
	) -> PF_Err {
		if cmd != PF_Cmd_SEQUENCE_SETUP {
			return PF_Err_NONE;
		}
		let mut n = 0;
		let mut project = std::ptr::null_mut();
		let mut depth_out = -1;
		let mut text = [0i8; AEGP_MAX_PROJ_NAME_SIZE as usize];
		let mut path_handle = std::ptr::null_mut();
		let mut path_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
		let memory = super::super::aegp_memory::suite_1();
		let ok = unsafe {
			count(&mut n) == 0
				&& by_index(0, &mut project) == 0
				&& name(project, text.as_mut_ptr()) == 0
				&& depth(project, &mut depth_out) == 0
				&& path(project, &mut path_handle) == 0
				&& memory.AEGP_LockMemHandle.unwrap()(path_handle, &mut path_ptr) == 0
				&& !path_ptr.is_null()
				&& *path_ptr.cast::<u16>() == 0
				&& memory.AEGP_UnlockMemHandle.unwrap()(path_handle) == 0
				&& memory.AEGP_FreeMemHandle.unwrap()(path_handle) == 0
		};
		OK.store(
			ok && n == 1
				&& depth_out == AEGP_ProjBitDepth_8 as AEGP_ProjBitDepth
				&& unsafe { std::ffi::CStr::from_ptr(text.as_ptr()) }.to_bytes() == b"aexlo",
			Ordering::SeqCst,
		);
		PF_Err_NONE
	}
	#[test]
	fn project_reports_current_host_and_rejects_invalid_index() {
		OK.store(false, Ordering::SeqCst);
		let instance = unsafe { crate::Host::get().entry_loader(entry) }.load().unwrap();
		assert!(OK.load(Ordering::SeqCst));
		let mut invalid = std::ptr::null_mut();
		assert_eq!(unsafe { by_index(1, &mut invalid) }, PF_Err_INVALID_INDEX);
		drop(instance);
	}
}
