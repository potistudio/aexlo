//! Typed bit flags and FourCCs the plugin declares, in place of the SDK's raw
//! integers (whose Rust type differs between the Windows and macOS bindings).

use core::fmt;
use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign};

/// Define a `u32` flag set whose named flags come from `crate::sdk`.
///
/// Unknown bits are kept, so a flag newer than this crate survives a round
/// trip through [`bits`](OutFlags::bits).
macro_rules! sdk_flags {
	(
		$(#[$meta:meta])*
		pub struct $name:ident;
		$($flag:ident = $sys:ident,)*
	) => {
		$(#[$meta])*
		#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
		pub struct $name(u32);

		impl $name {
			$(
				#[doc = concat!("`", stringify!($sys), "`")]
				pub const $flag: Self = Self(crate::sdk::$sys as u32);
			)*

			/// Every flag this crate names, with its name, for `Debug`.
			const NAMED: &[(&'static str, Self)] = &[$((stringify!($flag), Self::$flag),)*];

			/// No flags set.
			#[must_use]
			pub const fn empty() -> Self {
				Self(0)
			}

			/// The flags in `bits`, unknown bits included.
			#[must_use]
			pub const fn from_bits(bits: u32) -> Self {
				Self(bits)
			}

			/// The raw bits, as the SDK stores them.
			#[must_use]
			pub const fn bits(self) -> u32 {
				self.0
			}

			/// Whether every flag in `other` is set.
			#[must_use]
			pub const fn contains(self, other: Self) -> bool {
				self.0 & other.0 == other.0
			}

			/// Whether any flag in `other` is set.
			#[must_use]
			pub const fn intersects(self, other: Self) -> bool {
				self.0 & other.0 != 0
			}

			/// Whether no flag is set.
			#[must_use]
			pub const fn is_empty(self) -> bool {
				self.0 == 0
			}
		}

		impl BitOr for $name {
			type Output = Self;
			fn bitor(self, rhs: Self) -> Self {
				Self(self.0 | rhs.0)
			}
		}

		impl BitOrAssign for $name {
			fn bitor_assign(&mut self, rhs: Self) {
				self.0 |= rhs.0;
			}
		}

		impl BitAnd for $name {
			type Output = Self;
			fn bitand(self, rhs: Self) -> Self {
				Self(self.0 & rhs.0)
			}
		}

		impl BitAndAssign for $name {
			fn bitand_assign(&mut self, rhs: Self) {
				self.0 &= rhs.0;
			}
		}

		impl fmt::LowerHex for $name {
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				fmt::LowerHex::fmt(&self.0, f)
			}
		}

		impl fmt::Debug for $name {
			/// `Name(A | B | 0x40)`: the named flags, then any unknown bits.
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				write!(f, "{}(", stringify!($name))?;
				let mut rest = self.0;
				let mut first = true;
				for (name, flag) in Self::NAMED {
					if flag.0 != 0 && self.contains(*flag) {
						if !first {
							f.write_str(" | ")?;
						}
						f.write_str(name)?;
						rest &= !flag.0;
						first = false;
					}
				}
				if rest != 0 || first {
					if !first {
						f.write_str(" | ")?;
					}
					write!(f, "{rest:#x}")?;
				}
				f.write_str(")")
			}
		}
	};
}

sdk_flags! {
	/// `PF_OutFlags`: what an effect declares at `PF_Cmd_GLOBAL_SETUP`
	/// ([`PluginInstance::out_flags`](crate::PluginInstance::out_flags)).
	pub struct OutFlags;
	KEEP_RESOURCE_OPEN = PF_OutFlag_KEEP_RESOURCE_OPEN,
	WIDE_TIME_INPUT = PF_OutFlag_WIDE_TIME_INPUT,
	NON_PARAM_VARY = PF_OutFlag_NON_PARAM_VARY,
	SEQUENCE_DATA_NEEDS_FLATTENING = PF_OutFlag_SEQUENCE_DATA_NEEDS_FLATTENING,
	I_DO_DIALOG = PF_OutFlag_I_DO_DIALOG,
	USE_OUTPUT_EXTENT = PF_OutFlag_USE_OUTPUT_EXTENT,
	SEND_DO_DIALOG = PF_OutFlag_SEND_DO_DIALOG,
	DISPLAY_ERROR_MESSAGE = PF_OutFlag_DISPLAY_ERROR_MESSAGE,
	I_EXPAND_BUFFER = PF_OutFlag_I_EXPAND_BUFFER,
	PIX_INDEPENDENT = PF_OutFlag_PIX_INDEPENDENT,
	I_WRITE_INPUT_BUFFER = PF_OutFlag_I_WRITE_INPUT_BUFFER,
	I_SHRINK_BUFFER = PF_OutFlag_I_SHRINK_BUFFER,
	WORKS_IN_PLACE = PF_OutFlag_WORKS_IN_PLACE,
	CUSTOM_UI = PF_OutFlag_CUSTOM_UI,
	REFRESH_UI = PF_OutFlag_REFRESH_UI,
	NOP_RENDER = PF_OutFlag_NOP_RENDER,
	I_USE_SHUTTER_ANGLE = PF_OutFlag_I_USE_SHUTTER_ANGLE,
	I_USE_AUDIO = PF_OutFlag_I_USE_AUDIO,
	I_AM_OBSOLETE = PF_OutFlag_I_AM_OBSOLETE,
	FORCE_RERENDER = PF_OutFlag_FORCE_RERENDER,
	I_HAVE_EXTERNAL_DEPENDENCIES = PF_OutFlag_I_HAVE_EXTERNAL_DEPENDENCIES,
	DEEP_COLOR_AWARE = PF_OutFlag_DEEP_COLOR_AWARE,
	SEND_UPDATE_PARAMS_UI = PF_OutFlag_SEND_UPDATE_PARAMS_UI,
	AUDIO_FLOAT_ONLY = PF_OutFlag_AUDIO_FLOAT_ONLY,
	AUDIO_IIR = PF_OutFlag_AUDIO_IIR,
	I_SYNTHESIZE_AUDIO = PF_OutFlag_I_SYNTHESIZE_AUDIO,
	AUDIO_EFFECT_TOO = PF_OutFlag_AUDIO_EFFECT_TOO,
	AUDIO_EFFECT_ONLY = PF_OutFlag_AUDIO_EFFECT_ONLY,
}

sdk_flags! {
	/// `PF_OutFlags2`: the second set of flags an effect declares at
	/// `PF_Cmd_GLOBAL_SETUP`
	/// ([`PluginInstance::out_flags2`](crate::PluginInstance::out_flags2)).
	pub struct OutFlags2;
	SUPPORTS_QUERY_DYNAMIC_FLAGS = PF_OutFlag2_SUPPORTS_QUERY_DYNAMIC_FLAGS,
	I_USE_3D_CAMERA = PF_OutFlag2_I_USE_3D_CAMERA,
	I_USE_3D_LIGHTS = PF_OutFlag2_I_USE_3D_LIGHTS,
	PARAM_GROUP_START_COLLAPSED_FLAG = PF_OutFlag2_PARAM_GROUP_START_COLLAPSED_FLAG,
	I_AM_THREADSAFE = PF_OutFlag2_I_AM_THREADSAFE,
	CAN_COMBINE_WITH_DESTINATION = PF_OutFlag2_CAN_COMBINE_WITH_DESTINATION,
	DOESNT_NEED_EMPTY_PIXELS = PF_OutFlag2_DOESNT_NEED_EMPTY_PIXELS,
	REVEALS_ZERO_ALPHA = PF_OutFlag2_REVEALS_ZERO_ALPHA,
	PRESERVES_FULLY_OPAQUE_PIXELS = PF_OutFlag2_PRESERVES_FULLY_OPAQUE_PIXELS,
	SUPPORTS_SMART_RENDER = PF_OutFlag2_SUPPORTS_SMART_RENDER,
	FLOAT_COLOR_AWARE = PF_OutFlag2_FLOAT_COLOR_AWARE,
	I_USE_COLORSPACE_ENUMERATION = PF_OutFlag2_I_USE_COLORSPACE_ENUMERATION,
	I_AM_DEPRECATED = PF_OutFlag2_I_AM_DEPRECATED,
	PPRO_DO_NOT_CLONE_SEQUENCE_DATA_FOR_RENDER = PF_OutFlag2_PPRO_DO_NOT_CLONE_SEQUENCE_DATA_FOR_RENDER,
	AUTOMATIC_WIDE_TIME_INPUT = PF_OutFlag2_AUTOMATIC_WIDE_TIME_INPUT,
	I_USE_TIMECODE = PF_OutFlag2_I_USE_TIMECODE,
	DEPENDS_ON_UNREFERENCED_MASKS = PF_OutFlag2_DEPENDS_ON_UNREFERENCED_MASKS,
	OUTPUT_IS_WATERMARKED = PF_OutFlag2_OUTPUT_IS_WATERMARKED,
	I_MIX_GUID_DEPENDENCIES = PF_OutFlag2_I_MIX_GUID_DEPENDENCIES,
	AE13_5_THREADSAFE = PF_OutFlag2_AE13_5_THREADSAFE,
	SUPPORTS_GET_FLATTENED_SEQUENCE_DATA = PF_OutFlag2_SUPPORTS_GET_FLATTENED_SEQUENCE_DATA,
	CUSTOM_UI_ASYNC_MANAGER = PF_OutFlag2_CUSTOM_UI_ASYNC_MANAGER,
	SUPPORTS_GPU_RENDER_F32 = PF_OutFlag2_SUPPORTS_GPU_RENDER_F32,
	SUPPORTS_THREADED_RENDERING = PF_OutFlag2_SUPPORTS_THREADED_RENDERING,
	MUTABLE_RENDER_SEQUENCE_DATA_SLOWER = PF_OutFlag2_MUTABLE_RENDER_SEQUENCE_DATA_SLOWER,
	SUPPORTS_DIRECTX_RENDERING = PF_OutFlag2_SUPPORTS_DIRECTX_RENDERING,
}

sdk_flags! {
	/// `PF_ParamUIFlags` (`PF_PUI_*`): how a parameter is shown
	/// ([`PluginInstance::param_ui_flags`](crate::PluginInstance::param_ui_flags)).
	pub struct ParamUiFlags;
	TOPIC = PF_PUI_TOPIC,
	CONTROL = PF_PUI_CONTROL,
	STD_CONTROL_ONLY = PF_PUI_STD_CONTROL_ONLY,
	NO_ECW_UI = PF_PUI_NO_ECW_UI,
	ECW_SEPARATOR = PF_PUI_ECW_SEPARATOR,
	DISABLED = PF_PUI_DISABLED,
	DONT_ERASE_TOPIC = PF_PUI_DONT_ERASE_TOPIC,
	DONT_ERASE_CONTROL = PF_PUI_DONT_ERASE_CONTROL,
	RADIO_BUTTON = PF_PUI_RADIO_BUTTON,
	INVISIBLE = PF_PUI_INVISIBLE,
}

/// A pixel format FourCC (`PF_PixelFormat` / `PrPixelFormat`) a plugin
/// registered through the "PF Pixel Format Suite"
/// ([`PluginInstance::supported_pixel_formats`](crate::PluginInstance::supported_pixel_formats)).
///
/// Formats this crate doesn't name are kept as their FourCC.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PixelFormat(u32);

impl PixelFormat {
	/// `PF_PixelFormat_ARGB32`: 8 bpc ARGB.
	pub const ARGB32: Self = Self(crate::sdk::PF_PixelFormat_ARGB32 as u32);
	/// `PF_PixelFormat_ARGB64`: 16 bpc ARGB.
	pub const ARGB64: Self = Self(crate::sdk::PF_PixelFormat_ARGB64 as u32);
	/// `PF_PixelFormat_ARGB128`: 32 bpc float ARGB.
	pub const ARGB128: Self = Self(crate::sdk::PF_PixelFormat_ARGB128 as u32);
	/// `PF_PixelFormat_GPU_BGRA128`: 32 bpc float BGRA on the GPU.
	pub const GPU_BGRA128: Self = Self(crate::sdk::PF_PixelFormat_GPU_BGRA128 as u32);
	/// `PF_PixelFormat_BGRA32`: 8 bpc BGRA (Premiere).
	pub const BGRA32: Self = Self(crate::sdk::PF_PixelFormat_BGRA32 as u32);
	/// `PF_PixelFormat_VUYA32`: 8 bpc VUYA (Premiere).
	pub const VUYA32: Self = Self(crate::sdk::PF_PixelFormat_VUYA32 as u32);

	/// The format with FourCC `fourcc`.
	#[must_use]
	pub const fn from_fourcc(fourcc: u32) -> Self {
		Self(fourcc)
	}

	/// The FourCC, as the SDK stores it.
	#[must_use]
	pub const fn fourcc(self) -> u32 {
		self.0
	}
}

impl fmt::Debug for PixelFormat {
	/// `PixelFormat(ARGB128)` for named formats, the FourCC in hex otherwise.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let name = match *self {
			Self::ARGB32 => "ARGB32",
			Self::ARGB64 => "ARGB64",
			Self::ARGB128 => "ARGB128",
			Self::GPU_BGRA128 => "GPU_BGRA128",
			Self::BGRA32 => "BGRA32",
			Self::VUYA32 => "VUYA32",
			_ => return write!(f, "PixelFormat({:#010x})", self.0),
		};
		write!(f, "PixelFormat({name})")
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn flags_combine_and_keep_unknown_bits() {
		let flags = OutFlags::DEEP_COLOR_AWARE | OutFlags::I_DO_DIALOG;
		assert!(flags.contains(OutFlags::DEEP_COLOR_AWARE));
		assert!(!flags.contains(OutFlags::CUSTOM_UI));
		assert!(flags.intersects(OutFlags::I_DO_DIALOG | OutFlags::CUSTOM_UI));
		assert_eq!(OutFlags::from_bits(flags.bits()), flags);
		assert!(OutFlags::empty().is_empty());

		let unknown = OutFlags2::from_bits(OutFlags2::SUPPORTS_SMART_RENDER.bits() | (1 << 31));
		assert_eq!(unknown.bits() & (1 << 31), 1 << 31);
	}

	#[test]
	fn debug_names_known_flags_then_the_rest() {
		assert_eq!(format!("{:?}", ParamUiFlags::empty()), "ParamUiFlags(0x0)");
		assert_eq!(
			format!("{:?}", ParamUiFlags::CONTROL | ParamUiFlags::DISABLED),
			"ParamUiFlags(CONTROL | DISABLED)"
		);
		assert_eq!(
			format!("{:?}", ParamUiFlags::from_bits(ParamUiFlags::TOPIC.bits() | 1 << 20)),
			"ParamUiFlags(TOPIC | 0x100000)"
		);
		assert_eq!(format!("{:#x}", ParamUiFlags::CONTROL), "0x2");
	}

	#[test]
	fn pixel_formats_name_known_fourccs() {
		assert_eq!(format!("{:?}", PixelFormat::ARGB128), "PixelFormat(ARGB128)");
		assert_eq!(
			PixelFormat::from_fourcc(PixelFormat::ARGB32.fourcc()),
			PixelFormat::ARGB32
		);
		assert_eq!(format!("{:?}", PixelFormat::from_fourcc(1)), "PixelFormat(0x00000001)");
	}
}
