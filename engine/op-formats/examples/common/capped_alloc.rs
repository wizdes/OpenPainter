//! The fuzz workers' memory limits (shared by op-formats' `fuzz_worker` and
//! op-engine's `fuzz_engine_worker` through `#[path]`): a 2 GiB cap on Rust
//! heap allocations and a best-effort kernel limit. C allocations (LittleCMS)
//! do not pass through this allocator; `cargo xtask fuzz` also watches each
//! worker's resident size and kills it past 2 GiB.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

pub const MEMORY_LIMIT: usize = 2 << 30;
static IN_USE: AtomicUsize = AtomicUsize::new(0);

/// The system allocator with a hard cap: an allocation past the limit fails,
/// so the worker aborts instead of swapping the Mac.
struct Capped;
unsafe impl GlobalAlloc for Capped {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if IN_USE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size() > MEMORY_LIMIT {
            IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
            return std::ptr::null_mut();
        }
        let p = unsafe { System.alloc(layout) };
        if p.is_null() {
            IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if IN_USE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size() > MEMORY_LIMIT {
            IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
            return std::ptr::null_mut();
        }
        let p = unsafe { System.alloc_zeroed(layout) };
        if p.is_null() {
            IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = layout.size();
        if new_size > old
            && IN_USE.fetch_add(new_size - old, Ordering::Relaxed) + (new_size - old) > MEMORY_LIMIT
        {
            IN_USE.fetch_sub(new_size - old, Ordering::Relaxed);
            return std::ptr::null_mut();
        }
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if p.is_null() {
            if new_size > old {
                IN_USE.fetch_sub(new_size - old, Ordering::Relaxed);
            }
        } else if new_size < old {
            IN_USE.fetch_sub(old - new_size, Ordering::Relaxed);
        }
        p
    }
}
#[global_allocator]
static GLOBAL: Capped = Capped;

#[repr(C)]
struct RLimit {
    cur: u64,
    max: u64,
}
unsafe extern "C" {
    fn setrlimit(resource: i32, rlp: *const RLimit) -> i32;
}

/// Also asks the kernel for 2 GiB limits: RLIMIT_DATA, then RLIMIT_AS.
/// macOS refuses RLIMIT_AS and does not apply RLIMIT_DATA to mmap, so the
/// capped allocator above is the limit that holds; the result is logged.
pub fn set_rlimit() -> String {
    #[cfg(target_os = "macos")]
    const LIMITS: [(&str, i32); 2] = [("RLIMIT_DATA", 2), ("RLIMIT_AS", 5)];
    #[cfg(not(target_os = "macos"))]
    const LIMITS: [(&str, i32); 2] = [("RLIMIT_DATA", 2), ("RLIMIT_AS", 9)];
    let limit = RLimit {
        cur: MEMORY_LIMIT as u64,
        max: MEMORY_LIMIT as u64,
    };
    LIMITS
        .iter()
        .map(|(name, id)| {
            let ok = unsafe { setrlimit(*id, &limit) == 0 };
            format!("{name} {}", if ok { "set" } else { "refused" })
        })
        .collect::<Vec<_>>()
        .join(", ")
}
