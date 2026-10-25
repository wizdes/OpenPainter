//! M50 save options: `op_encode_image` encodes straight RGBA8 pixels as PNG,
//! JPEG or GIF with the save sheet's options, the resolution and the kept
//! metadata, into a buffer (the app writes the file and the size preview reads
//! the length). `op_import_pixels_at` is `op_import_pixels` with the file's
//! resolution.
use crate::{OpBuffer, OpEngine, OpStatus, bytes, engine, guard, json_bytes, owned, status};

fn optional<'a>(ptr: *const u8, len: usize) -> Result<Option<&'a [u8]>, OpStatus> {
    if len == 0 {
        return Ok(None);
    }
    bytes(ptr, len).map(Some)
}

/// Encodes `rgba` (`width` × `height` straight RGBA8, top row first) into
/// `out` (free with op_buffer_free). `json` holds the options, such as
/// `{"format":"jpeg","jpeg_quality":95,"jpeg_subsampling":"422","dpi":300}`;
/// see op_engine::save_export. `icc`, `exif` (a raw TIFF-structure EXIF block)
/// and `xmp` (an XMP packet) may be empty. Needs no engine or document.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn op_encode_image(
    json: *const u8,
    json_len: usize,
    width: u32,
    height: u32,
    rgba: *const u8,
    rgba_len: usize,
    icc: *const u8,
    icc_len: usize,
    exif: *const u8,
    exif_len: usize,
    xmp: *const u8,
    xmp_len: usize,
    out: *mut OpBuffer,
) -> OpStatus {
    guard(|| {
        if out.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        let run = || -> Result<Vec<u8>, OpStatus> {
            let params = json_bytes(json, json_len)?;
            let pixels = bytes(rgba, rgba_len)?;
            let icc = optional(icc, icc_len)?;
            let exif = optional(exif, exif_len)?;
            let xmp = optional(xmp, xmp_len)?;
            op_engine::save_export::encode_image(&params, width, height, pixels, icc, exif, xmp)
                .map_err(status)
        };
        match run() {
            Ok(v) => {
                unsafe { *out = owned(v) };
                OpStatus::OP_OK
            }
            Err(s) => s,
        }
    })
}

/// `op_import_pixels` at `dpi` pixels per inch (1 to 100000).
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn op_import_pixels_at(
    p: *mut OpEngine,
    width: u32,
    height: u32,
    rgba: *const u8,
    len: usize,
    icc: *const u8,
    icc_len: usize,
    dpi: f64,
    out_doc: *mut u64,
) -> OpStatus {
    guard(|| {
        if out_doc.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let data = match bytes(rgba, len) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let profile = match optional(icc, icc_len) {
            Ok(v) => v,
            Err(s) => return s,
        };
        match e.core().import_pixels_at(width, height, data, profile, dpi) {
            Ok(id) => {
                unsafe { *out_doc = id };
                OpStatus::OP_OK
            }
            Err(x) => status(x),
        }
    })
}
