//! Run one engine command on a PNG without the Mac app.
use op_engine::Engine;
use op_formats::png::{DecodedImage, decode_png, encode_png};
use serde_json::{Value, json};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!("Usage: opcli run <png> <command-id> [json] -o <out.png>");
        return Ok(());
    }
    if args[0] == "--version" || args[0] == "-V" {
        println!("opcli {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args[0] != "run" || args.len() < 5 {
        return Err("usage: opcli run <png> <command-id> [json] -o <out.png>".into());
    }
    let output = args.iter().position(|x| x == "-o").ok_or("missing -o")?;
    if output + 1 >= args.len() || output < 3 {
        return Err("missing output path".into());
    }
    let params: Value = if output == 4 {
        serde_json::from_str(&args[3])?
    } else {
        json!({})
    };
    let input = decode_png(&std::fs::read(&args[1])?)?;
    let engine = Engine::new(None)?;
    let doc = engine.import_pixels(
        input.width,
        input.height,
        &input.pixels,
        input.icc.as_deref(),
    )?;
    engine.dispatch(doc, &args[2], params)?;
    let pixels = engine.query(doc, "doc.pixels", json!({}))?;
    std::fs::write(
        &args[output + 1],
        encode_png(&DecodedImage {
            width: input.width,
            height: input.height,
            pixels,
            icc: input.icc,
        })?,
    )?;
    engine.close(doc)?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("opcli: {e}");
        std::process::exit(1)
    }
}
