//! CoreGraphics color spaces on a CAMetalLayer, and the lcms2 document -> display table.
use std::ffi::{c_char, c_void};

use objc::runtime::Object;
use objc::{msg_send, sel, sel_impl};
use op_gpu::presenter::Lut3d;

type CFTypeRef = *const c_void;
type CGColorSpaceRef = *mut c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataCreate(alloc: CFTypeRef, bytes: *const u8, len: isize) -> CFTypeRef;
    fn CFDataGetLength(data: CFTypeRef) -> isize;
    fn CFDataGetBytePtr(data: CFTypeRef) -> *const u8;
    fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: isize, encoding: u32) -> bool;
    fn CFRelease(obj: CFTypeRef);
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGColorSpaceCreateWithICCData(data: CFTypeRef) -> CGColorSpaceRef;
    fn CGColorSpaceCopyICCData(space: CGColorSpaceRef) -> CFTypeRef;
    fn CGColorSpaceCopyName(space: CGColorSpaceRef) -> CFTypeRef;
    fn CGColorSpaceRelease(space: CGColorSpaceRef);
}

const UTF8: u32 = 0x0800_0100;

/// An owned CGColorSpace made from ICC data.
pub struct ColorSpace(CGColorSpaceRef);

impl ColorSpace {
    pub fn from_icc(icc: &[u8]) -> Option<Self> {
        // SAFETY: CFDataCreate copies the bytes; both objects are released below or in Drop.
        unsafe {
            let data = CFDataCreate(std::ptr::null(), icc.as_ptr(), icc.len() as isize);
            if data.is_null() {
                return None;
            }
            let space = CGColorSpaceCreateWithICCData(data);
            CFRelease(data);
            (!space.is_null()).then_some(Self(space))
        }
    }
}

impl Drop for ColorSpace {
    fn drop(&mut self) {
        // SAFETY: we own one reference from CGColorSpaceCreateWithICCData.
        unsafe { CGColorSpaceRelease(self.0) }
    }
}

/// What a layer is tagged with right now.
pub struct LayerTag {
    pub icc: Option<Vec<u8>>,
    pub name: String,
}

/// Reads `CAMetalLayer.colorspace`.
///
/// # Safety
/// `layer` must be a live CAMetalLayer.
pub unsafe fn layer_tag(layer: *mut Object) -> LayerTag {
    // SAFETY: the getter returns a borrowed CGColorSpaceRef or null; the copies are released.
    unsafe {
        let space: CGColorSpaceRef = msg_send![layer, colorspace];
        if space.is_null() {
            return LayerTag {
                icc: None,
                name: "(none)".into(),
            };
        }
        let data = CGColorSpaceCopyICCData(space);
        let icc = (!data.is_null()).then(|| {
            let bytes =
                std::slice::from_raw_parts(CFDataGetBytePtr(data), CFDataGetLength(data) as usize)
                    .to_vec();
            CFRelease(data);
            bytes
        });
        let cfname = CGColorSpaceCopyName(space);
        let name = if cfname.is_null() {
            "(icc)".to_string()
        } else {
            let mut buf = [0 as c_char; 256];
            let ok = CFStringGetCString(cfname, buf.as_mut_ptr(), buf.len() as isize, UTF8);
            CFRelease(cfname);
            if ok {
                std::ffi::CStr::from_ptr(buf.as_ptr())
                    .to_string_lossy()
                    .into_owned()
            } else {
                "(unreadable)".into()
            }
        };
        LayerTag { icc, name }
    }
}

/// Sets `CAMetalLayer.colorspace` (the layer retains the space).
///
/// # Safety
/// `layer` must be a live CAMetalLayer.
pub unsafe fn set_layer_tag(layer: *mut Object, space: &ColorSpace) {
    // SAFETY: plain property setter.
    unsafe {
        let () = msg_send![layer, setColorspace: space.0];
    }
}

fn transform(
    doc_icc: &[u8],
    display_icc: &[u8],
) -> Result<lcms2::Transform<[f32; 3], [f32; 3]>, String> {
    use lcms2::{Intent, PixelFormat, Profile, Transform};
    let src = Profile::new_icc(doc_icc).map_err(|e| format!("document ICC: {e}"))?;
    let dst = Profile::new_icc(display_icc).map_err(|e| format!("display ICC: {e}"))?;
    Transform::new(
        &src,
        PixelFormat::RGB_FLT,
        &dst,
        PixelFormat::RGB_FLT,
        Intent::RelativeColorimetric,
    )
    .map_err(|e| format!("transform: {e}"))
}

/// lcms2 float transform of each color, clamped to 0..1.
pub fn convert_exact(
    doc_icc: &[u8],
    display_icc: &[u8],
    rgb: &[[f32; 3]],
) -> Result<Vec<[f32; 3]>, String> {
    let t = transform(doc_icc, display_icc)?;
    let mut out = vec![[0f32; 3]; rgb.len()];
    t.transform_pixels(rgb, &mut out);
    Ok(out
        .iter()
        .map(|c| {
            [
                c[0].clamp(0.0, 1.0),
                c[1].clamp(0.0, 1.0),
                c[2].clamp(0.0, 1.0),
            ]
        })
        .collect())
}

/// Document -> display table built with lcms2 (float transform, relative colorimetric,
/// no black point compensation), clamped to the display gamut.
pub fn build_lut(doc_icc: &[u8], display_icc: &[u8], n: u32) -> Result<Lut3d, String> {
    let t = transform(doc_icc, display_icc)?;
    let identity = Lut3d::identity(n);
    let input: Vec<[f32; 3]> = identity.data.iter().map(|c| [c[0], c[1], c[2]]).collect();
    let mut out = vec![[0f32; 3]; input.len()];
    t.transform_pixels(&input, &mut out);
    Ok(Lut3d {
        n,
        data: out
            .iter()
            .map(|c| {
                [
                    c[0].clamp(0.0, 1.0),
                    c[1].clamp(0.0, 1.0),
                    c[2].clamp(0.0, 1.0),
                    1.0,
                ]
            })
            .collect(),
    })
}
