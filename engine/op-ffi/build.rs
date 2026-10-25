fn main() {
    unsafe {
        std::env::set_var("CARGO_NET_OFFLINE", "true");
    }
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let header = std::path::Path::new(&crate_dir).join("include/openpainter.h");
    std::fs::create_dir_all(header.parent().unwrap()).unwrap();
    cbindgen::Builder::new()
        .with_config(cbindgen::Config {
            documentation: true,
            export: cbindgen::ExportConfig {
                // OpEvent::kind is a u32; export its values for C and Swift.
                include: vec!["OpEventKind".into()],
                ..Default::default()
            },
            ..Default::default()
        })
        .with_src(std::path::Path::new(&crate_dir).join("src/lib.rs"))
        .with_include_guard("OPENPAINTER_H")
        .with_header("/* Build order: mac/scripts/build-engine.sh before swift build.\n * Callbacks must copy event payloads and must not call op_*; re-entry returns OP_ERR_BUSY.\n * Inputs are borrowed for the call. Free outputs with op_buffer_free or op_pixels_free. */")
        .with_language(cbindgen::Language::C)
        .generate().expect("generate C header").write_to_file(header);
}
