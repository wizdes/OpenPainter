//! Panic-safe C interface. Foreign pointers must refer to valid buffers of their stated length.
#![allow(clippy::not_unsafe_ptr_arg_deref)]
use op_engine::{Engine, Error, Event, EventKind, JobTicket, Op};
use serde_json::{Value, json};
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
mod clipboard;
mod document;
mod export;
mod move_tool;
mod perf;
mod recovery;
mod stroke;
mod view;
pub use document::*;
pub use export::*;
pub use move_tool::*;
pub use perf::*;
pub use stroke::*;
pub use view::*;

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpStatus {
    OP_OK = 0,
    OP_ERR_BAD_ARGUMENT = 1,
    OP_ERR_NO_SUCH_DOC = 2,
    OP_ERR_READ_ONLY = 3,
    OP_ERR_PANIC = 4,
    OP_ERR_BUSY = 5,
    OP_ERR_INTERNAL = 6,
    OP_ERR_UNSUPPORTED = 7,
    /// The job was cancelled, or its result was stale and dropped. Nothing committed.
    OP_ERR_CANCELLED = 8,
}
/// Values of `OpEvent::kind`.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpEventKind {
    OP_EVENT_DOC_CHANGED = 1,
    OP_EVENT_HISTORY_CHANGED = 2,
    OP_EVENT_LAYERS_CHANGED = 3,
    /// Exactly one per ticket. Payload: `{"status":..., "code":<OpStatus>, "error"?:...}`.
    /// status is "ok", "cancelled", "dropped" (a stale preview render; code
    /// OP_ERR_CANCELLED) or an error name such as "read_only" or "panic".
    OP_EVENT_JOB_FINISHED = 4,
    OP_EVENT_PROGRESS = 5,
    /// M57: a recovery record was written, failed or was cleared. Payload:
    /// `{"ok":true,"revision":n}`, `{"ok":false,"error":"..."}` or
    /// `{"ok":true,"revision":null,"cleared":true}`.
    OP_EVENT_RECOVERY = 6,
}
#[repr(C)]
pub struct OpConfig {
    pub reserved: u32,
}
#[repr(C)]
pub struct OpBuffer {
    pub ptr: *mut u8,
    pub len: usize,
}
#[repr(C)]
pub struct OpPixelBuffer {
    pub ptr: *mut u8,
    pub len: usize,
    pub width: u32,
    pub height: u32,
}
/// One engine event. `json_ptr` is valid only during the callback; copy it.
#[repr(C)]
pub struct OpEvent {
    pub kind: u32,
    pub doc: u64,
    pub ticket: u64,
    pub json_ptr: *const u8,
    pub json_len: usize,
}
/// Runs on one engine thread, never re-entrantly. It must not call op_*; such a
/// call returns OP_ERR_BUSY (op_engine_new returns NULL, the free calls do nothing).
pub type OpEventFn = Option<unsafe extern "C" fn(*const OpEvent, *mut c_void)>;
#[repr(C)]
pub struct OpPointerEvent {
    pub x: f64,
    pub y: f64,
    pub pressure: f32,
    pub phase: u32,
}
thread_local! { static IN_CALLBACK:Cell<bool>=const {Cell::new(false)}; }
fn reentry() -> bool {
    IN_CALLBACK.with(Cell::get)
}
fn status(e: Error) -> OpStatus {
    match e {
        Error::BadArgument(_) => OpStatus::OP_ERR_BAD_ARGUMENT,
        Error::NoSuchDoc => OpStatus::OP_ERR_NO_SUCH_DOC,
        Error::ReadOnly => OpStatus::OP_ERR_READ_ONLY,
        Error::Panic => OpStatus::OP_ERR_PANIC,
        Error::Busy => OpStatus::OP_ERR_BUSY,
        Error::Internal(_) => OpStatus::OP_ERR_INTERNAL,
        Error::Cancelled => OpStatus::OP_ERR_CANCELLED,
    }
}
fn status_code(name: &str) -> OpStatus {
    match name {
        "ok" => OpStatus::OP_OK,
        "bad_argument" => OpStatus::OP_ERR_BAD_ARGUMENT,
        "no_such_doc" => OpStatus::OP_ERR_NO_SUCH_DOC,
        "read_only" => OpStatus::OP_ERR_READ_ONLY,
        "panic" => OpStatus::OP_ERR_PANIC,
        "busy" => OpStatus::OP_ERR_BUSY,
        "cancelled" | "dropped" => OpStatus::OP_ERR_CANCELLED,
        _ => OpStatus::OP_ERR_INTERNAL,
    }
}
fn guard(f: impl FnOnce() -> OpStatus) -> OpStatus {
    if reentry() {
        return OpStatus::OP_ERR_BUSY;
    }
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(OpStatus::OP_ERR_PANIC)
}
fn bytes<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], OpStatus> {
    if ptr.is_null() {
        return if len == 0 {
            Ok(&[])
        } else {
            Err(OpStatus::OP_ERR_BAD_ARGUMENT)
        };
    }
    if len > 1024 * 1024 * 1024 {
        return Err(OpStatus::OP_ERR_BAD_ARGUMENT);
    }
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
}
fn json_bytes(ptr: *const u8, len: usize) -> Result<Value, OpStatus> {
    if len == 0 {
        return Ok(json!({}));
    }
    serde_json::from_slice(bytes(ptr, len)?).map_err(|_| OpStatus::OP_ERR_BAD_ARGUMENT)
}
fn cstr<'a>(ptr: *const c_char) -> Result<&'a str, OpStatus> {
    if ptr.is_null() {
        return Err(OpStatus::OP_ERR_BAD_ARGUMENT);
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| OpStatus::OP_ERR_BAD_ARGUMENT)
}
fn owned(v: Vec<u8>) -> OpBuffer {
    let mut b = v.into_boxed_slice();
    let out = OpBuffer {
        ptr: b.as_mut_ptr(),
        len: b.len(),
    };
    std::mem::forget(b);
    out
}
type Tickets = Arc<Mutex<HashMap<u64, JobTicket>>>;
pub struct OpEngine {
    engine: Option<Engine>,
    /// Tickets without a JobFinished yet.
    tickets: Tickets,
    next_ticket: AtomicU64,
    callback_tx: Option<std::sync::mpsc::Sender<Event>>,
    callback_join: Mutex<Option<JoinHandle<()>>>,
}
fn make_event(e: Event) -> OpEvent {
    let kind = match e.kind {
        EventKind::DocChanged => OpEventKind::OP_EVENT_DOC_CHANGED,
        EventKind::HistoryChanged => OpEventKind::OP_EVENT_HISTORY_CHANGED,
        EventKind::LayersChanged => OpEventKind::OP_EVENT_LAYERS_CHANGED,
        EventKind::JobFinished => OpEventKind::OP_EVENT_JOB_FINISHED,
        EventKind::Progress => OpEventKind::OP_EVENT_PROGRESS,
        EventKind::Recovery => OpEventKind::OP_EVENT_RECOVERY,
    } as u32;
    OpEvent {
        kind,
        doc: e.doc,
        ticket: e.ticket,
        json_ptr: ptr::null(),
        json_len: 0,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn op_engine_new(
    _config: *const OpConfig,
    event_fn: OpEventFn,
    user: *mut c_void,
) -> *mut OpEngine {
    if reentry() {
        return ptr::null_mut();
    }
    catch_unwind(AssertUnwindSafe(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Event>();
        let user = user as usize;
        let join = std::thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                if let Some(cb) = event_fn {
                    let payload = event.json.to_string().into_bytes();
                    let mut raw = make_event(event);
                    raw.json_ptr = payload.as_ptr();
                    raw.json_len = payload.len();
                    IN_CALLBACK.with(|b| b.set(true));
                    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
                        cb(&raw, user as *mut c_void)
                    }));
                    IN_CALLBACK.with(|b| b.set(false));
                }
            }
        });
        let forward = Mutex::new(tx.clone());
        let tickets: Tickets = Arc::default();
        let open = tickets.clone();
        let engine = Engine::new(Some(Arc::new(move |mut e: Event| {
            if matches!(e.kind, EventKind::JobFinished) {
                open.lock().unwrap().remove(&e.ticket);
                let code = status_code(e.json["status"].as_str().unwrap_or(""));
                e.json["code"] = json!(code as u32);
            }
            let _ = forward.lock().unwrap().send(e);
        })))
        .ok()?;
        Some(Box::into_raw(Box::new(OpEngine {
            engine: Some(engine),
            tickets,
            next_ticket: AtomicU64::new(1),
            callback_tx: Some(tx),
            callback_join: Mutex::new(Some(join)),
        })))
    }))
    .ok()
    .flatten()
    .unwrap_or(ptr::null_mut())
}
/// Closes every document (queued operations and in-flight jobs finish first),
/// stops all engine threads and delivers the last events. No callback runs after
/// this returns.
#[unsafe(no_mangle)]
pub extern "C" fn op_engine_free(engine: *mut OpEngine) {
    if engine.is_null() || reentry() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // Canvas views go first: their render thread reads the engine.
        view::engine_freeing(engine);
        let mut e = unsafe { Box::from_raw(engine) };
        drop(e.engine.take());
        e.callback_tx.take();
        if let Some(j) = e.callback_join.get_mut().unwrap().take() {
            let _ = j.join();
        }
    }));
}
fn engine<'a>(p: *mut OpEngine) -> Result<&'a OpEngine, OpStatus> {
    if p.is_null() {
        Err(OpStatus::OP_ERR_BAD_ARGUMENT)
    } else {
        Ok(unsafe { &*p })
    }
}
impl OpEngine {
    fn core(&self) -> &Engine {
        self.engine
            .as_ref()
            .expect("engine lives until op_engine_free")
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn op_import_pixels(
    p: *mut OpEngine,
    width: u32,
    height: u32,
    rgba: *const u8,
    len: usize,
    icc: *const u8,
    icc_len: usize,
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
        let profile = if icc_len == 0 {
            None
        } else {
            match bytes(icc, icc_len) {
                Ok(v) => Some(v),
                Err(s) => return s,
            }
        };
        match e.core().import_pixels(width, height, data, profile) {
            Ok(id) => {
                unsafe { *out_doc = id };
                OpStatus::OP_OK
            }
            Err(x) => status(x),
        }
    })
}
/// The most bytes `op_doc_open_png` reads: a PNG whose pixels fit the 1 GiB
/// decode limit is smaller (a 16-bit one stored without compression is about
/// 2 GiB), so a larger file is refused before it is read (release review
/// item 7).
pub const MAX_PNG_FILE: u64 = 2 * 1024 * 1024 * 1024 + 16 * 1024 * 1024;

/// Reads a PNG file of at most `MAX_PNG_FILE` bytes.
fn read_png_file(path: &str) -> Result<Vec<u8>, OpStatus> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| OpStatus::OP_ERR_BAD_ARGUMENT)?;
    let size = file
        .metadata()
        .map_err(|_| OpStatus::OP_ERR_BAD_ARGUMENT)?
        .len();
    if size > MAX_PNG_FILE {
        return Err(OpStatus::OP_ERR_BAD_ARGUMENT);
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(MAX_PNG_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| OpStatus::OP_ERR_BAD_ARGUMENT)?;
    if bytes.len() as u64 > MAX_PNG_FILE {
        return Err(OpStatus::OP_ERR_BAD_ARGUMENT);
    }
    Ok(bytes)
}

#[unsafe(no_mangle)]
pub extern "C" fn op_doc_open_png(
    p: *mut OpEngine,
    path: *const u8,
    len: usize,
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
        let path = match bytes(path, len)
            .and_then(|b| std::str::from_utf8(b).map_err(|_| OpStatus::OP_ERR_BAD_ARGUMENT))
        {
            Ok(s) => s,
            Err(s) => return s,
        };
        match read_png_file(path).and_then(|b| e.core().open_png(&b).map_err(status)) {
            Ok(id) => {
                unsafe { *out_doc = id };
                OpStatus::OP_OK
            }
            Err(s) => s,
        }
    })
}
/// Queues a command on the document and returns at once with a ticket. Commands
/// on one document run in call order. `cmd_id` is a command id, or one of
/// "preview.begin" ({"command":id,"params":{...}}), "preview.update" (params),
/// "preview.commit", "preview.cancel". A bad id or bad parameters return
/// OP_ERR_BAD_ARGUMENT now; other failures arrive as the ticket's JobFinished.
#[unsafe(no_mangle)]
pub extern "C" fn op_dispatch(
    p: *mut OpEngine,
    doc: u64,
    cmd_id: *const c_char,
    json: *const u8,
    len: usize,
    out_ticket: *mut u64,
) -> OpStatus {
    guard(|| {
        if out_ticket.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        if e.core().snapshot(doc).is_err() {
            return OpStatus::OP_ERR_NO_SUCH_DOC;
        }
        let id = match cstr(cmd_id) {
            Ok(s) => s,
            Err(s) => return s,
        };
        let params = match json_bytes(json, len) {
            Ok(v) => v,
            Err(s) => return s,
        };
        let op = match id {
            "preview.begin" => {
                let Some(command) = params.get("command").and_then(Value::as_str) else {
                    return OpStatus::OP_ERR_BAD_ARGUMENT;
                };
                Op::PreviewBegin(
                    command.to_owned(),
                    params.get("params").cloned().unwrap_or(json!({})),
                )
            }
            "preview.update" => Op::PreviewUpdate(params),
            "preview.commit" => Op::PreviewCommit,
            "preview.cancel" => Op::PreviewCancel,
            _ => Op::Command(id.to_owned(), params),
        };
        let ticket = JobTicket::new(e.next_ticket.fetch_add(1, Ordering::Relaxed));
        // Registered before submit so its JobFinished always finds it.
        e.tickets
            .lock()
            .unwrap()
            .insert(ticket.id(), ticket.clone());
        match e.core().submit(doc, op, ticket.clone()) {
            Ok(()) => {
                unsafe { *out_ticket = ticket.id() };
                OpStatus::OP_OK
            }
            Err(x) => {
                e.tickets.lock().unwrap().remove(&ticket.id());
                status(x)
            }
        }
    })
}
/// Answers a query (JSON). The selection queries `doc.selection`,
/// `doc.selection_path` and `doc.selection_mask` take `{"preview":true}` to read
/// the open preview's selection (the live Magic Wand session, M31); without it,
/// or with no preview, they read the document.
#[unsafe(no_mangle)]
pub extern "C" fn op_query(
    p: *mut OpEngine,
    doc: u64,
    query_id: *const c_char,
    json: *const u8,
    len: usize,
    out: *mut OpBuffer,
) -> OpStatus {
    guard(|| {
        if out.is_null() {
            return OpStatus::OP_ERR_BAD_ARGUMENT;
        }
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        let id = match cstr(query_id) {
            Ok(s) => s,
            Err(s) => return s,
        };
        let params = match json_bytes(json, len) {
            Ok(v) => v,
            Err(s) => return s,
        };
        match e.core().query(doc, id, params) {
            Ok(data) => {
                unsafe { *out = owned(data) };
                OpStatus::OP_OK
            }
            Err(x) => status(x),
        }
    })
}
/// Cancels a ticket. Queued work is skipped; running work stops at its next check
/// and reports JobFinished with status "cancelled". Nothing it did is committed.
#[unsafe(no_mangle)]
pub extern "C" fn op_cancel(p: *mut OpEngine, ticket: u64) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        if let Some(t) = e.tickets.lock().unwrap().get(&ticket) {
            t.cancel();
            return OpStatus::OP_OK;
        }
        // A ticket that already finished: nothing left to stop.
        if ticket > 0 && ticket < e.next_ticket.load(Ordering::Relaxed) {
            OpStatus::OP_OK
        } else {
            OpStatus::OP_ERR_BAD_ARGUMENT
        }
    })
}
/// Waits for the document's queued operations and in-flight jobs, then frees it.
#[unsafe(no_mangle)]
pub extern "C" fn op_doc_close(p: *mut OpEngine, doc: u64) -> OpStatus {
    guard(|| {
        let e = match engine(p) {
            Ok(e) => e,
            Err(s) => return s,
        };
        // The document's canvas views stop and release their layers first.
        view::doc_closing(p, doc);
        status_result(e.core().close(doc))
    })
}
fn status_result(r: op_engine::Result<()>) -> OpStatus {
    r.map(|_| OpStatus::OP_OK).unwrap_or_else(status)
}
#[unsafe(no_mangle)]
pub extern "C" fn op_buffer_free(b: *mut OpBuffer) {
    if b.is_null() || reentry() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        let b = &mut *b;
        if !b.ptr.is_null() {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                b.ptr, b.len,
            )));
            b.ptr = ptr::null_mut();
            b.len = 0;
        }
    }));
}
#[unsafe(no_mangle)]
pub extern "C" fn op_pixels_free(b: *mut OpPixelBuffer) {
    if b.is_null() || reentry() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        let b = &mut *b;
        if !b.ptr.is_null() {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                b.ptr, b.len,
            )));
            b.ptr = ptr::null_mut();
            b.len = 0;
        }
    }));
}
#[unsafe(no_mangle)]
pub extern "C" fn op_pointer(
    _p: *mut OpEngine,
    _view: u64,
    _events: *const OpPointerEvent,
    _len: usize,
) -> OpStatus {
    guard(|| OpStatus::OP_ERR_UNSUPPORTED)
}
