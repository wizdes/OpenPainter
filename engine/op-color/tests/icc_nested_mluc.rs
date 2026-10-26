// Fix round 2, item 15: a profile whose `meta` tag is a dictionary with a
// display name that is an mluc body claiming about 35 million records. lcms2
// allocates 12 bytes per claimed record (about 420 MB) when it reads that
// tag. `Profile::from_icc` checks only top-level mluc tags, so this test
// shows whether anything OpenPainter does with a profile reads the
// dictionary: it counts every allocation lcms2 makes (a memory plugin) while
// the profile is opened, described and used for a conversion.
//
// This file holds one test: the memory plugin is process-wide.
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

static LARGEST: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
    fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
}

extern "C" fn count_malloc(_: *mut c_void, size: u32) -> *mut c_void {
    LARGEST.fetch_max(u64::from(size), Ordering::Relaxed);
    // SAFETY: plain C malloc; lcms2 frees it through `count_free`.
    unsafe { malloc(size as usize) }
}
extern "C" fn count_free(_: *mut c_void, ptr: *mut c_void) {
    // SAFETY: every pointer lcms2 frees came from malloc or realloc.
    unsafe { free(ptr) }
}
extern "C" fn count_realloc(_: *mut c_void, ptr: *mut c_void, size: u32) -> *mut c_void {
    LARGEST.fetch_max(u64::from(size), Ordering::Relaxed);
    // SAFETY: as count_free.
    unsafe { realloc(ptr, size as usize) }
}

/// lcms2's `cmsPluginMemHandler` (lcms2_plugin.h).
#[repr(C)]
struct PluginBase {
    magic: u32,
    expected_version: u32,
    kind: u32,
    next: *mut PluginBase,
}
#[repr(C)]
struct MemHandler {
    base: PluginBase,
    malloc: extern "C" fn(*mut c_void, u32) -> *mut c_void,
    free: extern "C" fn(*mut c_void, *mut c_void),
    realloc: extern "C" fn(*mut c_void, *mut c_void, u32) -> *mut c_void,
    malloc_zero: *const c_void,
    calloc: *const c_void,
    dup: *const c_void,
}

fn install_counter() {
    let plugin = Box::leak(Box::new(MemHandler {
        base: PluginBase {
            magic: 0x6163_7070, // 'acpp'
            expected_version: 2000,
            kind: 0x6D65_6D48, // 'memH'
            next: std::ptr::null_mut(),
        },
        malloc: count_malloc,
        free: count_free,
        realloc: count_realloc,
        malloc_zero: std::ptr::null(),
        calloc: std::ptr::null(),
        dup: std::ptr::null(),
    }));
    // SAFETY: the plugin lives for the whole process (leaked).
    let ok = unsafe { lcms2_sys::cmsPlugin((plugin as *mut MemHandler).cast()) };
    assert_ne!(ok, 0, "the memory plugin installs");
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}

/// The built-in sRGB profile with one more tag: `meta`, a dictionary whose
/// one entry has a display name mluc that claims `records` records.
fn crafted(records: u32) -> Vec<u8> {
    let base = op_color::Profile::builtin("srgb").unwrap().bytes().to_vec();
    let count = be32(&base, 128) as usize;
    let old_table_end = 132 + count * 12;
    // The dictionary: header, one 24-byte record (name, value, display name),
    // the name "a" and value "b" (UTF-16, padded to 4), then the display name.
    let mut dict = Vec::new();
    dict.extend_from_slice(b"dict");
    dict.extend_from_slice(&[0; 4]);
    dict.extend_from_slice(&1u32.to_be_bytes());
    dict.extend_from_slice(&24u32.to_be_bytes());
    let name_at = 16 + 24;
    let value_at = name_at + 4;
    let display_at = value_at + 4;
    for (off, len) in [(name_at, 2u32), (value_at, 2), (display_at, 20)] {
        dict.extend_from_slice(&(off as u32).to_be_bytes());
        dict.extend_from_slice(&len.to_be_bytes());
    }
    dict.extend_from_slice(&[0, b'a', 0, 0, 0, b'b', 0, 0]);
    // lcms2 reads (and writes) a dictionary's display name as the body of
    // an mluc with no "mluc" signature in front: the record count first.
    dict.extend_from_slice(&records.to_be_bytes());
    dict.extend_from_slice(&12u32.to_be_bytes());
    dict.extend_from_slice(b"enUS");
    dict.extend_from_slice(&2u32.to_be_bytes());
    dict.extend_from_slice(&28u32.to_be_bytes());
    // Rebuild: header, one more table entry (every offset moves by 12), the
    // old tag data, then the dictionary, 4-byte aligned.
    let mut out = base[..128].to_vec();
    out.extend_from_slice(&((count + 1) as u32).to_be_bytes());
    for i in 0..count {
        let e = 132 + i * 12;
        out.extend_from_slice(&base[e..e + 4]);
        out.extend_from_slice(&(be32(&base, e + 4) + 12).to_be_bytes());
        out.extend_from_slice(&base[e + 8..e + 12]);
    }
    let mut data = base[old_table_end..].to_vec();
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
    let dict_at = 132 + (count + 1) * 12 + data.len();
    out.extend_from_slice(b"meta");
    out.extend_from_slice(&(dict_at as u32).to_be_bytes());
    out.extend_from_slice(&(dict.len() as u32).to_be_bytes());
    out.extend_from_slice(&data);
    out.extend_from_slice(&dict);
    let len = out.len() as u32;
    out[..4].copy_from_slice(&len.to_be_bytes());
    out
}

#[test]
fn an_mluc_inside_a_dictionary_is_never_read_by_opening_or_using_a_profile() {
    install_counter();
    let bytes = crafted(35_000_000);
    // The control: reading the dictionary itself makes lcms2 ask for
    // about 420 MB, so the crafted profile is armed.
    let raw = lcms2::Profile::new_icc(&bytes).expect("lcms2 opens the crafted profile");
    LARGEST.store(0, Ordering::Relaxed);
    let _ = raw.read_tag(lcms2::TagSignature::MetaTag);
    let control = LARGEST.load(Ordering::Relaxed);
    assert!(
        control >= 400_000_000,
        "reading the dictionary allocates {control} bytes"
    );
    drop(raw);
    // What OpenPainter does with an embedded profile: open it (the safety
    // checks and the description), then convert pixels to and from sRGB.
    LARGEST.store(0, Ordering::Relaxed);
    let p = op_color::Profile::from_icc(&bytes).expect("the profile is otherwise valid");
    assert!(!p.description().is_empty());
    let srgb = op_color::Profile::builtin("srgb").unwrap();
    let px = [[10u8, 200, 30], [255, 0, 128]];
    op_color::convert_rgb(&p, &srgb, op_color::RenderingIntent::Perceptual, false, &px).unwrap();
    op_color::convert_rgb(&srgb, &p, op_color::RenderingIntent::Relative, true, &px).unwrap();
    op_color::display_table(&p, &srgb, 9).unwrap();
    let used = LARGEST.load(Ordering::Relaxed);
    assert!(
        used < 64 * 1024 * 1024,
        "the largest lcms2 allocation was {used} bytes"
    );
}
