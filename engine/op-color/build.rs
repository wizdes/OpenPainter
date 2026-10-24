fn main() {
    println!("cargo:rerun-if-env-changed=LCMS2_LIB_DIR");
    if std::env::var_os("LCMS2_LIB_DIR").is_some() {
        panic!("LCMS2_LIB_DIR overrides vendored static lcms2; unset it before building");
    }
}
