//! Drawbot suites that rasterise into a pixel buffer.
//!
//! Custom-UI effects paint during `PF_Event_DRAW`. The host opens a frame,
//! `PF_GetDrawingReference` hands back that frame, and the suites below record
//! pens, brushes, paths and images and stamp them into the buffer. The same
//! path serves every effect that draws its own control; nothing here is
//! specific to one plugin.
//!
//! Text uses a common sans-serif face installed with the operating system.
//! When none of those files is present, `SupportsText` reports false and
//! `DrawString` does nothing.

use after_effects_sys::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use ab_glyph::{Font, FontVec, ScaleFont};
use tiny_skia::{
	FillRule, FilterQuality, LineCap, LineJoin, Mask, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, StrokeDash,
	Transform,
};

const MAX_EDGE: u32 = 2048;

struct Heap {
	next: usize,
	objects: HashMap<usize, Object>,
	/// Session id of the frame currently being drawn, or 0.
	current: usize,
}

struct Object {
	refs: u32,
	kind: Kind,
}

enum Kind {
	Session { surface: usize },
	Surface(Surface),
	Pen(Pen),
	Brush(Rgba),
	Path(Vec<Verb>),
	Image(Image),
	Font { size: f32 },
}

struct Surface {
	pixmap: Pixmap,
	transform: Transform,
	clip: PixelRect,
	stack: Vec<SurfaceState>,
	anti_alias: bool,
	interpolation: DRAWBOT_InterpolationPolicy,
}

struct SurfaceState {
	transform: Transform,
	clip: PixelRect,
	anti_alias: bool,
	interpolation: DRAWBOT_InterpolationPolicy,
}

struct Pen {
	color: Rgba,
	width: f32,
	dashes: Vec<f32>,
}

struct Image {
	pixmap: Pixmap,
	scale: f32,
}

#[derive(Clone, Copy)]
struct Rgba {
	red: f32,
	green: f32,
	blue: f32,
	alpha: f32,
}

#[derive(Clone, Copy)]
struct PixelRect {
	x: i32,
	y: i32,
	width: i32,
	height: i32,
}

#[derive(Clone)]
enum Verb {
	Move(f32, f32),
	Line(f32, f32),
	Cubic(f32, f32, f32, f32, f32, f32),
	Close,
}

/// Straight RGBA pixels produced by one `PF_Event_DRAW`.
#[derive(Clone)]
pub(crate) struct DrawnFrame {
	pub width: u32,
	pub height: u32,
	pub rgba: Vec<u8>,
}

fn with_heap<T>(f: impl FnOnce(&mut Heap) -> T) -> T {
	thread_local! {
		static HEAP: RefCell<Heap> = RefCell::new(Heap {
			next: 0x1000,
			objects: HashMap::new(),
			current: 0,
		});
	}
	HEAP.with(|heap| f(&mut heap.borrow_mut()))
}

fn alloc(heap: &mut Heap, kind: Kind) -> usize {
	let id = heap.next;
	heap.next = heap.next.wrapping_add(1).max(0x1000);
	heap.objects.insert(id, Object { refs: 1, kind });
	id
}

fn id_of(ptr: *mut std::ffi::c_void) -> usize {
	ptr as usize
}

fn insert(kind: Kind) -> usize {
	with_heap(|heap| alloc(heap, kind))
}

/// Open a transparent buffer for the control rectangle of one draw event.
pub(crate) fn begin_frame(width: u32, height: u32) -> bool {
	let width = width.clamp(1, MAX_EDGE);
	let height = height.clamp(1, MAX_EDGE);
	let Some(pixmap) = Pixmap::new(width, height) else {
		log::warn!("drawbot: could not allocate a {width}x{height} surface");
		return false;
	};
	with_heap(|heap| {
		let surface = alloc(
			heap,
			Kind::Surface(Surface {
				pixmap,
				transform: Transform::identity(),
				clip: PixelRect {
					x: 0,
					y: 0,
					width: width as i32,
					height: height as i32,
				},
				stack: Vec::new(),
				anti_alias: true,
				interpolation: kDRAWBOT_InterpolationPolicy_None,
			}),
		);
		let session = alloc(heap, Kind::Session { surface });
		heap.current = session;
	});
	true
}

/// Draw ref for the frame opened by [`begin_frame`], if one is open.
pub(crate) fn current_draw_ref() -> Option<DRAWBOT_DrawRef> {
	with_heap(|heap| {
		let id = heap.current;
		if id == 0 || !heap.objects.contains_key(&id) {
			None
		} else {
			Some(id as DRAWBOT_DrawRef)
		}
	})
}

/// Finish the current frame and return its straight-alpha pixels.
pub(crate) fn end_frame() -> Option<DrawnFrame> {
	with_heap(|heap| {
		let session = heap.current;
		heap.current = 0;
		if session == 0 {
			return None;
		}
		let surface_id = match heap.objects.get(&session) {
			Some(Object {
				kind: Kind::Session { surface },
				..
			}) => *surface,
			_ => {
				heap.objects.remove(&session);
				return None;
			}
		};
		let frame = heap.objects.get(&surface_id).and_then(|object| match &object.kind {
			Kind::Surface(surface) => Some(unpremultiply(surface)),
			_ => None,
		});
		heap.objects.remove(&surface_id);
		heap.objects.remove(&session);
		frame
	})
}

fn unpremultiply(surface: &Surface) -> DrawnFrame {
	let width = surface.pixmap.width();
	let height = surface.pixmap.height();
	let mut rgba = Vec::with_capacity((width * height * 4) as usize);
	for pixel in surface.pixmap.pixels() {
		let alpha = pixel.alpha();
		if alpha == 0 {
			rgba.extend_from_slice(&[0, 0, 0, 0]);
			continue;
		}
		let scale = 255.0 / alpha as f32;
		rgba.push((pixel.red() as f32 * scale).round().min(255.0) as u8);
		rgba.push((pixel.green() as f32 * scale).round().min(255.0) as u8);
		rgba.push((pixel.blue() as f32 * scale).round().min(255.0) as u8);
		rgba.push(alpha);
	}
	DrawnFrame { width, height, rgba }
}

fn with_surface<T>(surface: DRAWBOT_SurfaceRef, f: impl FnOnce(&mut Surface) -> T) -> Option<T> {
	with_heap(|heap| {
		let object = heap.objects.get_mut(&id_of(surface.cast()))?;
		match &mut object.kind {
			Kind::Surface(surface) => Some(f(surface)),
			_ => None,
		}
	})
}

fn with_path(path: DRAWBOT_PathRef, f: impl FnOnce(&mut Vec<Verb>)) -> SPErr {
	with_heap(|heap| {
		let Some(object) = heap.objects.get_mut(&id_of(path.cast())) else {
			return 4;
		};
		match &mut object.kind {
			Kind::Path(verbs) => {
				f(verbs);
				0
			}
			_ => 4,
		}
	})
}

fn read_color(color: *const DRAWBOT_ColorRGBA) -> Option<Rgba> {
	let color = unsafe { color.as_ref() }?;
	Some(Rgba {
		red: color.red.clamp(0.0, 1.0),
		green: color.green.clamp(0.0, 1.0),
		blue: color.blue.clamp(0.0, 1.0),
		alpha: color.alpha.clamp(0.0, 1.0),
	})
}

fn paint_of(color: Rgba, anti_alias: bool) -> Paint<'static> {
	let mut paint = Paint::default();
	paint.anti_alias = anti_alias;
	if let Some(packed) = tiny_skia::Color::from_rgba(color.red, color.green, color.blue, color.alpha) {
		paint.set_color(packed);
	}
	paint
}

fn clip_mask(surface: &Surface) -> Option<Mask> {
	let full = surface.clip.x <= 0
		&& surface.clip.y <= 0
		&& surface.clip.width >= surface.pixmap.width() as i32
		&& surface.clip.height >= surface.pixmap.height() as i32;
	if full {
		return None;
	}
	let mut mask = Mask::new(surface.pixmap.width(), surface.pixmap.height())?;
	let mut builder = PathBuilder::new();
	if let Some(rect) = tiny_skia::Rect::from_xywh(
		surface.clip.x as f32,
		surface.clip.y as f32,
		surface.clip.width.max(0) as f32,
		surface.clip.height.max(0) as f32,
	) {
		builder.push_rect(rect);
	}
	let Some(path) = builder.finish() else {
		return Some(mask);
	};
	mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
	Some(mask)
}

fn replay(verbs: &[Verb]) -> Option<tiny_skia::Path> {
	let mut builder = PathBuilder::new();
	for verb in verbs {
		match *verb {
			Verb::Move(x, y) => builder.move_to(x, y),
			Verb::Line(x, y) => builder.line_to(x, y),
			Verb::Cubic(x1, y1, x2, y2, x, y) => builder.cubic_to(x1, y1, x2, y2, x, y),
			Verb::Close => builder.close(),
		}
	}
	builder.finish()
}

fn map_point(transform: Transform, x: f32, y: f32) -> (f32, f32) {
	(
		transform.sx * x + transform.kx * y + transform.tx,
		transform.ky * x + transform.sy * y + transform.ty,
	)
}

/// Adobe stores the affine matrix as rows `(sx, kx, 0)`, `(ky, sy, 0)`, `(tx, ty, 1)`.
fn from_drawbot_matrix(matrix: &DRAWBOT_MatrixF32) -> Transform {
	Transform::from_row(
		matrix.mat[0][0],
		matrix.mat[1][0],
		matrix.mat[0][1],
		matrix.mat[1][1],
		matrix.mat[2][0],
		matrix.mat[2][1],
	)
}

fn intersect_clip(current: PixelRect, next: PixelRect, bounds_w: i32, bounds_h: i32) -> PixelRect {
	let x0 = current.x.max(next.x).max(0);
	let y0 = current.y.max(next.y).max(0);
	let x1 = (current.x + current.width).min(next.x + next.width).min(bounds_w);
	let y1 = (current.y + current.height).min(next.y + next.height).min(bounds_h);
	PixelRect {
		x: x0,
		y: y0,
		width: (x1 - x0).max(0),
		height: (y1 - y0).max(0),
	}
}

fn device_rect(transform: Transform, rect: &DRAWBOT_Rect32, bounds_w: i32, bounds_h: i32) -> PixelRect {
	let left = rect.left as f32;
	let top = rect.top as f32;
	let right = left + rect.width as f32;
	let bottom = top + rect.height as f32;
	let corners = [
		map_point(transform, left, top),
		map_point(transform, right, top),
		map_point(transform, left, bottom),
		map_point(transform, right, bottom),
	];
	let min_x = corners.iter().map(|p| p.0).fold(f32::INFINITY, f32::min).floor() as i32;
	let min_y = corners.iter().map(|p| p.1).fold(f32::INFINITY, f32::min).floor() as i32;
	let max_x = corners.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max).ceil() as i32;
	let max_y = corners.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max).ceil() as i32;
	PixelRect {
		x: min_x.clamp(0, bounds_w),
		y: min_y.clamp(0, bounds_h),
		width: (max_x - min_x).clamp(0, bounds_w),
		height: (max_y - min_y).clamp(0, bounds_h),
	}
}

fn system_font() -> Option<&'static FontVec> {
	static FONT: OnceLock<Option<FontVec>> = OnceLock::new();
	FONT.get_or_init(|| {
		let candidates = [
			r"C:\Windows\Fonts\arial.ttf",
			r"C:\Windows\Fonts\segoeui.ttf",
			"/System/Library/Fonts/Supplemental/Arial.ttf",
			"/Library/Fonts/Arial.ttf",
			"/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
		];
		for path in candidates {
			let Ok(bytes) = std::fs::read(path) else {
				continue;
			};
			if let Ok(font) = FontVec::try_from_vec(bytes) {
				return Some(font);
			}
		}
		log::info!("drawbot: no system sans-serif found; text drawing is disabled");
		None
	})
	.as_ref()
}

fn read_utf16(text: *const DRAWBOT_UTF16Char) -> String {
	if text.is_null() {
		return String::new();
	}
	let mut len = 0usize;
	while len < 4096 && unsafe { *text.add(len) } != 0 {
		len += 1;
	}
	String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) })
}

fn draw_text(
	surface: &mut Surface,
	color: Rgba,
	size: f32,
	text: &str,
	origin: (f32, f32),
	alignment: i32,
	truncation: i32,
	truncation_width: f32,
) {
	let Some(font) = system_font() else {
		return;
	};
	let Some(scale) = font.pt_to_px_scale(size.max(1.0)) else {
		return;
	};
	let scaled = font.as_scaled(scale);
	let mut glyphs = Vec::new();
	let mut caret = 0.0f32;
	for ch in text.chars() {
		let glyph = scaled.scaled_glyph(ch);
		let advance = scaled.h_advance(glyph.id);
		if truncation != kDRAWBOT_TextTruncation_None && truncation_width > 0.0 && caret + advance > truncation_width {
			break;
		}
		glyphs.push((glyph, caret));
		caret += advance;
	}
	let shift = match alignment {
		x if x == kDRAWBOT_TextAlignment_Center => -caret * 0.5,
		x if x == kDRAWBOT_TextAlignment_Right => -caret,
		_ => 0.0,
	};
	let (origin_x, origin_y) = map_point(surface.transform, origin.0 + shift, origin.1);
	let scale_x = (surface.transform.sx.abs() + surface.transform.kx.abs()).max(0.01);
	let scale_y = (surface.transform.sy.abs() + surface.transform.ky.abs()).max(0.01);
	let mask = clip_mask(surface);
	for (mut glyph, x) in glyphs {
		glyph.position = ab_glyph::point(origin_x + x * scale_x, origin_y);
		glyph.scale = ab_glyph::PxScale {
			x: glyph.scale.x * scale_x,
			y: glyph.scale.y * scale_y,
		};
		let Some(outlined) = font.outline_glyph(glyph) else {
			continue;
		};
		let bounds = outlined.px_bounds();
		outlined.draw(|gx, gy, coverage| {
			blend_coverage(
				&mut surface.pixmap,
				&mask,
				bounds.min.x as i32 + gx as i32,
				bounds.min.y as i32 + gy as i32,
				coverage,
				color,
			);
		});
	}
}

fn blend_coverage(pixmap: &mut Pixmap, mask: &Option<Mask>, x: i32, y: i32, coverage: f32, color: Rgba) {
	if x < 0 || y < 0 || x as u32 >= pixmap.width() || y as u32 >= pixmap.height() {
		return;
	}
	if let Some(mask) = mask
		&& mask.data()[(y as u32 * pixmap.width() + x as u32) as usize] == 0
	{
		return;
	}
	let index = (y as u32 * pixmap.width() + x as u32) as usize;
	let dest = pixmap.pixels()[index];
	let src_a = (coverage.clamp(0.0, 1.0) * color.alpha).clamp(0.0, 1.0);
	let inv = 1.0 - src_a;
	// Source and destination are combined in premultiplied space.
	let red = color.red * src_a + (dest.red() as f32 / 255.0) * inv;
	let green = color.green * src_a + (dest.green() as f32 / 255.0) * inv;
	let blue = color.blue * src_a + (dest.blue() as f32 / 255.0) * inv;
	let alpha = src_a + (dest.alpha() as f32 / 255.0) * inv;
	let straight_a = alpha.max(f32::EPSILON);
	if let Some(packed) = tiny_skia::Color::from_rgba(red / straight_a, green / straight_a, blue / straight_a, alpha) {
		pixmap.pixels_mut()[index] = packed.premultiply().to_color_u8();
	}
}

fn image_from_buffer(
	width: i32,
	height: i32,
	row_bytes: i32,
	layout: DRAWBOT_PixelLayout,
	data: *const std::ffi::c_void,
) -> Option<Pixmap> {
	if width <= 0 || height <= 0 || data.is_null() || row_bytes <= 0 {
		return None;
	}
	let width = width as u32;
	let height = height as u32;
	if width > MAX_EDGE || height > MAX_EDGE {
		return None;
	}
	let mut pixmap = Pixmap::new(width, height)?;
	let channels: usize = if layout == kDRAWBOT_PixelLayout_24RGB || layout == kDRAWBOT_PixelLayout_24BGR {
		3
	} else {
		4
	};
	let src = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), row_bytes as usize * height as usize) };
	for y in 0..height as usize {
		let row = &src[y * row_bytes as usize..];
		for x in 0..width as usize {
			let pixel = &row[x * channels..];
			let (r, g, b, a) = match layout {
				kDRAWBOT_PixelLayout_24RGB | kDRAWBOT_PixelLayout_32RGB => (pixel[0], pixel[1], pixel[2], 255),
				kDRAWBOT_PixelLayout_24BGR | kDRAWBOT_PixelLayout_32BGR => (pixel[2], pixel[1], pixel[0], 255),
				kDRAWBOT_PixelLayout_32ARGB_Straight => (pixel[1], pixel[2], pixel[3], pixel[0]),
				kDRAWBOT_PixelLayout_32ARGB_Premul => unpremul_bytes(pixel[1], pixel[2], pixel[3], pixel[0]),
				kDRAWBOT_PixelLayout_32BGRA_Straight => (pixel[2], pixel[1], pixel[0], pixel[3]),
				kDRAWBOT_PixelLayout_32BGRA_Premul => unpremul_bytes(pixel[2], pixel[1], pixel[0], pixel[3]),
				_ => (0, 0, 0, 0),
			};
			let color = tiny_skia::Color::from_rgba8(r, g, b, a);
			pixmap.pixels_mut()[y * width as usize + x] = color.premultiply().to_color_u8();
		}
	}
	Some(pixmap)
}

fn unpremul_bytes(red: u8, green: u8, blue: u8, alpha: u8) -> (u8, u8, u8, u8) {
	if alpha == 0 {
		return (0, 0, 0, 0);
	}
	let scale = 255.0 / alpha as f32;
	(
		(red as f32 * scale).round().min(255.0) as u8,
		(green as f32 * scale).round().min(255.0) as u8,
		(blue as f32 * scale).round().min(255.0) as u8,
		alpha,
	)
}

fn add_arc(verbs: &mut Vec<Verb>, center: (f32, f32), radius: f32, start: f32, sweep: f32) {
	if radius <= 0.0 || sweep == 0.0 {
		return;
	}
	let mut angle = start;
	let mut remaining = sweep;
	let direction = sweep.signum();
	let point = |radians: f32| (center.0 + radius * radians.cos(), center.1 + radius * radians.sin());
	let (x, y) = point(start);
	if verbs.is_empty() {
		verbs.push(Verb::Move(x, y));
	} else {
		verbs.push(Verb::Line(x, y));
	}
	while remaining.abs() > 0.0001 {
		let step = remaining.abs().min(std::f32::consts::FRAC_PI_2) * direction;
		let kappa = (4.0 / 3.0) * (step / 4.0).tan();
		let (x0, y0) = point(angle);
		let (x1, y1) = point(angle + step);
		let c1x = x0 - kappa * radius * angle.sin();
		let c1y = y0 + kappa * radius * angle.cos();
		let end = angle + step;
		let c2x = x1 + kappa * radius * end.sin();
		let c2y = y1 - kappa * radius * end.cos();
		verbs.push(Verb::Cubic(c1x, c1y, c2x, c2y, x1, y1));
		angle = end;
		remaining -= step;
	}
}

unsafe extern "C" fn get_supplier(in_drawbot_ref: DRAWBOT_DrawRef, out_supplier: *mut DRAWBOT_SupplierRef) -> SPErr {
	let _ = in_drawbot_ref;
	if let Some(slot) = unsafe { out_supplier.as_mut() } {
		*slot = 1usize as DRAWBOT_SupplierRef;
	}
	0
}

unsafe extern "C" fn get_surface(in_drawbot_ref: DRAWBOT_DrawRef, out_surface: *mut DRAWBOT_SurfaceRef) -> SPErr {
	let surface = with_heap(|heap| match heap.objects.get(&id_of(in_drawbot_ref.cast())) {
		Some(Object {
			kind: Kind::Session { surface },
			..
		}) => Some(*surface),
		_ => None,
	});
	let Some(surface) = surface else {
		return 4;
	};
	if let Some(slot) = unsafe { out_surface.as_mut() } {
		*slot = surface as DRAWBOT_SurfaceRef;
	}
	0
}

unsafe extern "C" fn new_pen(
	supplier: DRAWBOT_SupplierRef,
	color: *const DRAWBOT_ColorRGBA,
	size: f32,
	out_pen: *mut DRAWBOT_PenRef,
) -> SPErr {
	let _ = supplier;
	let Some(color) = read_color(color) else {
		return 4;
	};
	let id = insert(Kind::Pen(Pen {
		color,
		width: if size <= 0.0 { 1.0 } else { size },
		dashes: Vec::new(),
	}));
	if let Some(slot) = unsafe { out_pen.as_mut() } {
		*slot = id as DRAWBOT_PenRef;
	}
	0
}

unsafe extern "C" fn new_brush(
	supplier: DRAWBOT_SupplierRef,
	color: *const DRAWBOT_ColorRGBA,
	out_brush: *mut DRAWBOT_BrushRef,
) -> SPErr {
	let _ = supplier;
	let Some(color) = read_color(color) else {
		return 4;
	};
	let id = insert(Kind::Brush(color));
	if let Some(slot) = unsafe { out_brush.as_mut() } {
		*slot = id as DRAWBOT_BrushRef;
	}
	0
}

unsafe extern "C" fn supports_text(supplier: DRAWBOT_SupplierRef, out_supports: *mut DRAWBOT_Boolean) -> SPErr {
	let _ = supplier;
	if let Some(out) = unsafe { out_supports.as_mut() } {
		*out = u8::from(system_font().is_some());
	}
	0
}

unsafe extern "C" fn default_font_size(supplier: DRAWBOT_SupplierRef, out_size: *mut f32) -> SPErr {
	let _ = supplier;
	if let Some(out) = unsafe { out_size.as_mut() } {
		*out = 12.0;
	}
	0
}

unsafe extern "C" fn new_font(supplier: DRAWBOT_SupplierRef, size: f32, out_font: *mut DRAWBOT_FontRef) -> SPErr {
	let _ = supplier;
	let id = insert(Kind::Font { size: size.max(1.0) });
	if let Some(slot) = unsafe { out_font.as_mut() } {
		*slot = id as DRAWBOT_FontRef;
	}
	0
}

unsafe extern "C" fn new_image(
	supplier: DRAWBOT_SupplierRef,
	width: i32,
	height: i32,
	row_bytes: i32,
	layout: DRAWBOT_PixelLayout,
	data: *const std::ffi::c_void,
	out_image: *mut DRAWBOT_ImageRef,
) -> SPErr {
	let _ = supplier;
	let Some(pixmap) = image_from_buffer(width, height, row_bytes, layout, data) else {
		return 4;
	};
	let id = insert(Kind::Image(Image { pixmap, scale: 1.0 }));
	if let Some(slot) = unsafe { out_image.as_mut() } {
		*slot = id as DRAWBOT_ImageRef;
	}
	0
}

unsafe extern "C" fn new_path(supplier: DRAWBOT_SupplierRef, out_path: *mut DRAWBOT_PathRef) -> SPErr {
	let _ = supplier;
	let id = insert(Kind::Path(Vec::new()));
	if let Some(slot) = unsafe { out_path.as_mut() } {
		*slot = id as DRAWBOT_PathRef;
	}
	0
}

unsafe extern "C" fn flag(out_flag: *mut DRAWBOT_Boolean, value: u8) -> SPErr {
	if let Some(out) = unsafe { out_flag.as_mut() } {
		*out = value;
	}
	0
}

unsafe extern "C" fn supports_bgra(supplier: DRAWBOT_SupplierRef, out_flag: *mut DRAWBOT_Boolean) -> SPErr {
	let _ = supplier;
	flag(out_flag, 1)
}

unsafe extern "C" fn prefers_bgra(supplier: DRAWBOT_SupplierRef, out_flag: *mut DRAWBOT_Boolean) -> SPErr {
	let _ = supplier;
	flag(out_flag, 1)
}

unsafe extern "C" fn supports_argb(supplier: DRAWBOT_SupplierRef, out_flag: *mut DRAWBOT_Boolean) -> SPErr {
	let _ = supplier;
	flag(out_flag, 1)
}

unsafe extern "C" fn prefers_argb(supplier: DRAWBOT_SupplierRef, out_flag: *mut DRAWBOT_Boolean) -> SPErr {
	let _ = supplier;
	flag(out_flag, 0)
}

unsafe extern "C" fn retain(object: DRAWBOT_ObjectRef) -> SPErr {
	let found = with_heap(|heap| {
		let Some(object) = heap.objects.get_mut(&id_of(object.cast())) else {
			return false;
		};
		object.refs = object.refs.saturating_add(1);
		true
	});
	if found { 0 } else { 4 }
}

unsafe extern "C" fn release(object: DRAWBOT_ObjectRef) -> SPErr {
	with_heap(|heap| {
		let id = id_of(object.cast());
		let Some(stored) = heap.objects.get_mut(&id) else {
			return 4;
		};
		// The frame and its surface stay until [`end_frame`]. Plugins only own
		// the objects they created with `New*`.
		if matches!(stored.kind, Kind::Session { .. } | Kind::Surface(_)) {
			return 0;
		}
		stored.refs = stored.refs.saturating_sub(1);
		if stored.refs == 0 {
			heap.objects.remove(&id);
		}
		0
	})
}

unsafe extern "C" fn push_state(surface: DRAWBOT_SurfaceRef) -> SPErr {
	with_surface(surface, |surface| {
		surface.stack.push(SurfaceState {
			transform: surface.transform,
			clip: surface.clip,
			anti_alias: surface.anti_alias,
			interpolation: surface.interpolation,
		});
	});
	0
}

unsafe extern "C" fn pop_state(surface: DRAWBOT_SurfaceRef) -> SPErr {
	with_surface(surface, |surface| {
		if let Some(state) = surface.stack.pop() {
			surface.transform = state.transform;
			surface.clip = state.clip;
			surface.anti_alias = state.anti_alias;
			surface.interpolation = state.interpolation;
		}
	});
	0
}

unsafe extern "C" fn paint_rect(
	surface: DRAWBOT_SurfaceRef,
	color: *const DRAWBOT_ColorRGBA,
	rect: *const DRAWBOT_RectF32,
) -> SPErr {
	let (Some(color), Some(rect)) = (read_color(color), (unsafe { rect.as_ref() })) else {
		return 4;
	};
	with_surface(surface, |surface| {
		let Some(rect) = tiny_skia::Rect::from_xywh(rect.left, rect.top, rect.width, rect.height) else {
			return;
		};
		let mut builder = PathBuilder::new();
		builder.push_rect(rect);
		let Some(path) = builder.finish() else {
			return;
		};
		let mask = clip_mask(surface);
		let paint = paint_of(color, surface.anti_alias);
		surface
			.pixmap
			.fill_path(&path, &paint, FillRule::Winding, surface.transform, mask.as_ref());
	});
	0
}

unsafe extern "C" fn fill_path(
	surface: DRAWBOT_SurfaceRef,
	brush: DRAWBOT_BrushRef,
	path: DRAWBOT_PathRef,
	fill: DRAWBOT_FillType,
) -> SPErr {
	let Some((color, verbs)) = with_heap(|heap| {
		let color = match heap.objects.get(&id_of(brush.cast())) {
			Some(Object {
				kind: Kind::Brush(color),
				..
			}) => *color,
			_ => return None,
		};
		let verbs = match heap.objects.get(&id_of(path.cast())) {
			Some(Object {
				kind: Kind::Path(verbs),
				..
			}) => verbs.clone(),
			_ => return None,
		};
		Some((color, verbs))
	}) else {
		return 4;
	};
	let Some(path) = replay(&verbs) else {
		return 0;
	};
	let rule = if fill == kDRAWBOT_FillType_EvenOdd {
		FillRule::EvenOdd
	} else {
		FillRule::Winding
	};
	with_surface(surface, |surface| {
		let mask = clip_mask(surface);
		let paint = paint_of(color, surface.anti_alias);
		surface
			.pixmap
			.fill_path(&path, &paint, rule, surface.transform, mask.as_ref());
	});
	0
}

unsafe extern "C" fn stroke_path(surface: DRAWBOT_SurfaceRef, pen: DRAWBOT_PenRef, path: DRAWBOT_PathRef) -> SPErr {
	let Some((pen, verbs)) = with_heap(|heap| {
		let pen = match heap.objects.get(&id_of(pen.cast())) {
			Some(Object {
				kind: Kind::Pen(pen), ..
			}) => Pen {
				color: pen.color,
				width: pen.width,
				dashes: pen.dashes.clone(),
			},
			_ => return None,
		};
		let verbs = match heap.objects.get(&id_of(path.cast())) {
			Some(Object {
				kind: Kind::Path(verbs),
				..
			}) => verbs.clone(),
			_ => return None,
		};
		Some((pen, verbs))
	}) else {
		return 4;
	};
	let Some(path) = replay(&verbs) else {
		return 0;
	};
	with_surface(surface, |surface| {
		let mask = clip_mask(surface);
		let paint = paint_of(pen.color, surface.anti_alias);
		let dash = if pen.dashes.is_empty() {
			None
		} else {
			StrokeDash::new(pen.dashes.clone(), 0.0)
		};
		let stroke = Stroke {
			width: pen.width,
			miter_limit: 4.0,
			line_cap: LineCap::Butt,
			line_join: LineJoin::Miter,
			dash,
		};
		surface
			.pixmap
			.stroke_path(&path, &paint, &stroke, surface.transform, mask.as_ref());
	});
	0
}

unsafe extern "C" fn clip(
	surface: DRAWBOT_SurfaceRef,
	supplier: DRAWBOT_SupplierRef,
	rect: *const DRAWBOT_Rect32,
) -> SPErr {
	let _ = supplier;
	let Some(rect) = (unsafe { rect.as_ref() }) else {
		return 4;
	};
	with_surface(surface, |surface| {
		let bounds_w = surface.pixmap.width() as i32;
		let bounds_h = surface.pixmap.height() as i32;
		let next = device_rect(surface.transform, rect, bounds_w, bounds_h);
		surface.clip = intersect_clip(surface.clip, next, bounds_w, bounds_h);
	});
	0
}

unsafe extern "C" fn clip_bounds(surface: DRAWBOT_SurfaceRef, out_rect: *mut DRAWBOT_Rect32) -> SPErr {
	let Some(rect) = with_surface(surface, |surface| surface.clip) else {
		return 4;
	};
	if let Some(out) = unsafe { out_rect.as_mut() } {
		*out = DRAWBOT_Rect32 {
			left: rect.x,
			top: rect.y,
			width: rect.width,
			height: rect.height,
		};
	}
	0
}

unsafe extern "C" fn within_clip(
	surface: DRAWBOT_SurfaceRef,
	rect: *const DRAWBOT_Rect32,
	out_within: *mut DRAWBOT_Boolean,
) -> SPErr {
	let Some(rect) = (unsafe { rect.as_ref() }) else {
		return 4;
	};
	let inside = with_surface(surface, |surface| {
		let bounds_w = surface.pixmap.width() as i32;
		let bounds_h = surface.pixmap.height() as i32;
		let next = device_rect(surface.transform, rect, bounds_w, bounds_h);
		next.x >= surface.clip.x
			&& next.y >= surface.clip.y
			&& next.x + next.width <= surface.clip.x + surface.clip.width
			&& next.y + next.height <= surface.clip.y + surface.clip.height
	})
	.unwrap_or(false);
	flag(out_within, u8::from(inside))
}

unsafe extern "C" fn transform(surface: DRAWBOT_SurfaceRef, matrix: *const DRAWBOT_MatrixF32) -> SPErr {
	let Some(matrix) = (unsafe { matrix.as_ref() }) else {
		return 4;
	};
	let incoming = from_drawbot_matrix(matrix);
	with_surface(surface, |surface| {
		surface.transform = surface.transform.pre_concat(incoming);
	});
	0
}

unsafe extern "C" fn draw_string(
	surface: DRAWBOT_SurfaceRef,
	brush: DRAWBOT_BrushRef,
	font: DRAWBOT_FontRef,
	text: *const DRAWBOT_UTF16Char,
	origin: *const DRAWBOT_PointF32,
	alignment: DRAWBOT_TextAlignment,
	truncation: DRAWBOT_TextTruncation,
	truncation_width: f32,
) -> SPErr {
	let Some((color, size)) = with_heap(|heap| {
		let color = match heap.objects.get(&id_of(brush.cast())) {
			Some(Object {
				kind: Kind::Brush(color),
				..
			}) => *color,
			_ => return None,
		};
		let size = match heap.objects.get(&id_of(font.cast())) {
			Some(Object {
				kind: Kind::Font { size },
				..
			}) => *size,
			_ => return None,
		};
		Some((color, size))
	}) else {
		return 4;
	};
	let Some(origin) = (unsafe { origin.as_ref() }) else {
		return 4;
	};
	let text = read_utf16(text);
	with_surface(surface, |surface| {
		draw_text(
			surface,
			color,
			size,
			&text,
			(origin.x, origin.y),
			alignment,
			truncation,
			truncation_width,
		);
	});
	0
}

unsafe extern "C" fn draw_image(
	surface: DRAWBOT_SurfaceRef,
	image: DRAWBOT_ImageRef,
	origin: *const DRAWBOT_PointF32,
	alpha: f32,
) -> SPErr {
	let Some((pixmap, scale)) = with_heap(|heap| match heap.objects.get(&id_of(image.cast())) {
		Some(Object {
			kind: Kind::Image(image),
			..
		}) => Some((image.pixmap.clone(), image.scale.max(0.01))),
		_ => None,
	}) else {
		return 4;
	};
	let Some(origin) = (unsafe { origin.as_ref() }) else {
		return 4;
	};
	with_surface(surface, |surface| {
		let mask = clip_mask(surface);
		let mut paint = PixmapPaint::default();
		paint.opacity = alpha.clamp(0.0, 1.0);
		paint.quality = if surface.interpolation == kDRAWBOT_InterpolationPolicy_None {
			FilterQuality::Nearest
		} else {
			FilterQuality::Bilinear
		};
		let placement = Transform::from_row(1.0 / scale, 0.0, 0.0, 1.0 / scale, origin.x, origin.y);
		surface.pixmap.draw_pixmap(
			0,
			0,
			pixmap.as_ref(),
			&paint,
			surface.transform.pre_concat(placement),
			mask.as_ref(),
		);
	});
	0
}

unsafe extern "C" fn set_interp(surface: DRAWBOT_SurfaceRef, policy: DRAWBOT_InterpolationPolicy) -> SPErr {
	with_surface(surface, |surface| surface.interpolation = policy);
	0
}

unsafe extern "C" fn get_interp(surface: DRAWBOT_SurfaceRef, out_interp: *mut DRAWBOT_InterpolationPolicy) -> SPErr {
	let Some(policy) = with_surface(surface, |surface| surface.interpolation) else {
		return 4;
	};
	if let Some(out) = unsafe { out_interp.as_mut() } {
		*out = policy;
	}
	0
}

unsafe extern "C" fn set_aa(surface: DRAWBOT_SurfaceRef, policy: DRAWBOT_AntiAliasPolicy) -> SPErr {
	with_surface(surface, |surface| {
		surface.anti_alias = policy != kDRAWBOT_AntiAliasPolicy_None;
	});
	0
}

unsafe extern "C" fn get_aa(surface: DRAWBOT_SurfaceRef, out_policy: *mut DRAWBOT_AntiAliasPolicy) -> SPErr {
	let Some(anti_alias) = with_surface(surface, |surface| surface.anti_alias) else {
		return 4;
	};
	if let Some(out) = unsafe { out_policy.as_mut() } {
		*out = if anti_alias {
			kDRAWBOT_AntiAliasPolicy_High
		} else {
			kDRAWBOT_AntiAliasPolicy_None
		};
	}
	0
}

unsafe extern "C" fn flush(surface: DRAWBOT_SurfaceRef) -> SPErr {
	let _ = surface;
	0
}

unsafe extern "C" fn screen_scale(surface: DRAWBOT_SurfaceRef, out_scale: *mut f32) -> SPErr {
	let _ = surface;
	if let Some(out) = unsafe { out_scale.as_mut() } {
		*out = 1.0;
	}
	0
}

unsafe extern "C" fn path_move(path: DRAWBOT_PathRef, x: f32, y: f32) -> SPErr {
	with_path(path, |verbs| verbs.push(Verb::Move(x, y)))
}

unsafe extern "C" fn path_line(path: DRAWBOT_PathRef, x: f32, y: f32) -> SPErr {
	with_path(path, |verbs| verbs.push(Verb::Line(x, y)))
}

unsafe extern "C" fn bezier(
	path: DRAWBOT_PathRef,
	p1: *const DRAWBOT_PointF32,
	p2: *const DRAWBOT_PointF32,
	p3: *const DRAWBOT_PointF32,
) -> SPErr {
	let (Some(p1), Some(p2), Some(p3)) = (unsafe { p1.as_ref() }, unsafe { p2.as_ref() }, unsafe { p3.as_ref() })
	else {
		return 4;
	};
	with_path(path, |verbs| {
		verbs.push(Verb::Cubic(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y))
	})
}

unsafe extern "C" fn add_rect(path: DRAWBOT_PathRef, rect: *const DRAWBOT_RectF32) -> SPErr {
	let Some(rect) = (unsafe { rect.as_ref() }) else {
		return 4;
	};
	let right = rect.left + rect.width;
	let bottom = rect.top + rect.height;
	with_path(path, |verbs| {
		verbs.push(Verb::Move(rect.left, rect.top));
		verbs.push(Verb::Line(right, rect.top));
		verbs.push(Verb::Line(right, bottom));
		verbs.push(Verb::Line(rect.left, bottom));
		verbs.push(Verb::Close);
	})
}

unsafe extern "C" fn path_arc(
	path: DRAWBOT_PathRef,
	center: *const DRAWBOT_PointF32,
	radius: f32,
	start: f32,
	sweep: f32,
) -> SPErr {
	let Some(center) = (unsafe { center.as_ref() }) else {
		return 4;
	};
	with_path(path, |verbs| add_arc(verbs, (center.x, center.y), radius, start, sweep))
}

unsafe extern "C" fn path_close(path: DRAWBOT_PathRef) -> SPErr {
	with_path(path, |verbs| verbs.push(Verb::Close))
}

unsafe extern "C" fn set_dash(pen: DRAWBOT_PenRef, dashes: *const f32, pattern_size: i32) -> SPErr {
	if pattern_size < 0 || (pattern_size > 0 && dashes.is_null()) {
		return 4;
	}
	let pattern = if pattern_size == 0 {
		Vec::new()
	} else {
		unsafe { std::slice::from_raw_parts(dashes, pattern_size as usize) }.to_vec()
	};
	let updated = with_heap(|heap| {
		let Some(object) = heap.objects.get_mut(&id_of(pen.cast())) else {
			return false;
		};
		match &mut object.kind {
			Kind::Pen(pen) => {
				pen.dashes = pattern;
				true
			}
			_ => false,
		}
	});
	if updated { 0 } else { 4 }
}

unsafe extern "C" fn set_scale(image: DRAWBOT_ImageRef, scale: f32) -> SPErr {
	let updated = with_heap(|heap| {
		let Some(object) = heap.objects.get_mut(&id_of(image.cast())) else {
			return false;
		};
		match &mut object.kind {
			Kind::Image(image) => {
				image.scale = if scale <= 0.0 { 1.0 } else { scale };
				true
			}
			_ => false,
		}
	});
	if updated { 0 } else { 4 }
}

pub(super) static DRAW_SUITE: DRAWBOT_DrawbotSuite1 = DRAWBOT_DrawbotSuite1 {
	GetSupplier: Some(get_supplier),
	GetSurface: Some(get_surface),
};

pub(super) static SUPPLIER_SUITE: DRAWBOT_SupplierSuite1 = DRAWBOT_SupplierSuite1 {
	NewPen: Some(new_pen),
	NewBrush: Some(new_brush),
	SupportsText: Some(supports_text),
	GetDefaultFontSize: Some(default_font_size),
	NewDefaultFont: Some(new_font),
	NewImageFromBuffer: Some(new_image),
	NewPath: Some(new_path),
	SupportsPixelLayoutBGRA: Some(supports_bgra),
	PrefersPixelLayoutBGRA: Some(prefers_bgra),
	SupportsPixelLayoutARGB: Some(supports_argb),
	PrefersPixelLayoutARGB: Some(prefers_argb),
	RetainObject: Some(retain),
	ReleaseObject: Some(release),
};

pub(super) static SURFACE_SUITE: DRAWBOT_SurfaceSuite2 = DRAWBOT_SurfaceSuite1 {
	PushStateStack: Some(push_state),
	PopStateStack: Some(pop_state),
	PaintRect: Some(paint_rect),
	FillPath: Some(fill_path),
	StrokePath: Some(stroke_path),
	Clip: Some(clip),
	GetClipBounds: Some(clip_bounds),
	IsWithinClipBounds: Some(within_clip),
	Transform: Some(transform),
	DrawString: Some(draw_string),
	DrawImage: Some(draw_image),
	SetInterpolationPolicy: Some(set_interp),
	GetInterpolationPolicy: Some(get_interp),
	SetAntiAliasPolicy: Some(set_aa),
	GetAntiAliasPolicy: Some(get_aa),
	Flush: Some(flush),
	GetTransformToScreenScale: Some(screen_scale),
};

pub(super) static PATH_SUITE: DRAWBOT_PathSuite1 = DRAWBOT_PathSuite1 {
	MoveTo: Some(path_move),
	LineTo: Some(path_line),
	BezierTo: Some(bezier),
	AddRect: Some(add_rect),
	AddArc: Some(path_arc),
	Close: Some(path_close),
};

pub(super) static PEN_SUITE: DRAWBOT_PenSuite1 = DRAWBOT_PenSuite1 {
	SetDashPattern: Some(set_dash),
};

pub(super) static IMAGE_SUITE: DRAWBOT_ImageSuite1 = DRAWBOT_ImageSuite1 {
	SetScaleFactor: Some(set_scale),
};

#[cfg(test)]
mod tests {
	use super::*;

	fn surface_of(draw: DRAWBOT_DrawRef) -> DRAWBOT_SurfaceRef {
		let mut supplier = std::ptr::null_mut();
		let mut surface = std::ptr::null_mut();
		unsafe {
			get_supplier(draw, &mut supplier);
			get_surface(draw, &mut surface);
		}
		surface
	}

	#[test]
	fn paint_rect_fills_straight_red() {
		assert!(begin_frame(4, 4));
		let draw = current_draw_ref().expect("frame");
		let color = DRAWBOT_ColorRGBA {
			red: 1.0,
			green: 0.0,
			blue: 0.0,
			alpha: 1.0,
		};
		let rect = DRAWBOT_RectF32 {
			left: 0.0,
			top: 0.0,
			width: 4.0,
			height: 4.0,
		};
		assert_eq!(unsafe { paint_rect(surface_of(draw), &color, &rect) }, 0);
		let frame = end_frame().expect("pixels");
		assert_eq!(frame.rgba[0..4], [255, 0, 0, 255]);
		assert!(current_draw_ref().is_none());
	}

	#[test]
	fn fill_path_paints_the_interior() {
		assert!(begin_frame(8, 8));
		let draw = current_draw_ref().expect("frame");
		let surface = surface_of(draw);
		let mut supplier = std::ptr::null_mut();
		unsafe { get_supplier(draw, &mut supplier) };
		let color = DRAWBOT_ColorRGBA {
			red: 0.0,
			green: 0.0,
			blue: 1.0,
			alpha: 1.0,
		};
		let mut brush = std::ptr::null_mut();
		let mut path = std::ptr::null_mut();
		unsafe {
			new_brush(supplier, &color, &mut brush);
			new_path(supplier, &mut path);
			path_move(path, 1.0, 1.0);
			path_line(path, 7.0, 1.0);
			path_line(path, 7.0, 7.0);
			path_close(path);
			fill_path(surface, brush, path, kDRAWBOT_FillType_Winding);
			release(brush.cast());
			release(path.cast());
		}
		let frame = end_frame().expect("pixels");
		let inside = ((3 * 8 + 6) * 4) as usize;
		assert_eq!(frame.rgba[inside + 2], 255, "blue channel");
		assert_eq!(frame.rgba[3], 0, "the corner outside the triangle stays clear");
	}
}
