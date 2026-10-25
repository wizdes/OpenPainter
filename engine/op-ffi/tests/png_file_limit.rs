// Release review item 7: op_doc_open_png refuses a file larger than any PNG
// the decoder accepts before reading it into memory.
use op_ffi::*;
use std::ptr;

#[test]
fn an_oversized_png_file_is_refused_before_it_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("huge.png");
    let file = std::fs::File::create(&path).unwrap();
    std::io::Write::write_all(&mut &file, b"\x89PNG\r\n\x1a\n").unwrap();
    // Sparse: no disk space is used, but reading it all would take 2 GiB.
    file.set_len(MAX_PNG_FILE + 1).unwrap();
    let e = op_engine_new(ptr::null(), None, ptr::null_mut());
    let bytes = path.to_str().unwrap().as_bytes();
    let mut id = 0u64;
    let t = std::time::Instant::now();
    let status = op_doc_open_png(e, bytes.as_ptr(), bytes.len(), &mut id);
    assert_eq!(status, OpStatus::OP_ERR_BAD_ARGUMENT);
    assert!(
        t.elapsed() < std::time::Duration::from_millis(200),
        "{:?}",
        t.elapsed()
    );
    op_engine_free(e);
}
