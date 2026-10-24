#[test]
fn executable_has_no_dynamic_lcms_or_homebrew_link() {
    let src = op_color::Profile::builtin("srgb").unwrap();
    let dst = op_color::Profile::builtin("display-p3").unwrap();
    let _ = op_color::convert_rgb(
        &src,
        &dst,
        op_color::RenderingIntent::Relative,
        true,
        &[[25, 100, 200]],
    )
    .unwrap();
    let exe = std::env::current_exe().unwrap();
    let (program, arg) = if cfg!(target_os = "macos") {
        ("otool", "-L")
    } else if cfg!(target_os = "linux") {
        ("ldd", "")
    } else {
        return;
    };
    let mut command = std::process::Command::new(program);
    if !arg.is_empty() {
        command.arg(arg);
    }
    let output = command
        .arg(&exe)
        .output()
        .expect("dependency inspection tool must exist");
    assert!(
        output.status.success(),
        "{} failed: {}",
        program,
        String::from_utf8_lossy(&output.stderr)
    );
    let libraries = String::from_utf8_lossy(&output.stdout);
    println!("{} {}:\n{}", program, exe.display(), libraries);
    for line in libraries.lines() {
        let lower = line.to_ascii_lowercase();
        assert!(
            !lower.contains("/opt/homebrew")
                && !lower.contains("/usr/local")
                && !lower.contains("liblcms"),
            "forbidden dynamic library: {line}"
        );
    }
}
