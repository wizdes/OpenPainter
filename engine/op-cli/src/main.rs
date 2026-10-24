//! Run engine commands and scenarios from the command line.

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None | Some("--help" | "-h") => {
            println!(
                "opcli {}\nUsage: opcli [--help | --version]",
                env!("CARGO_PKG_VERSION")
            );
        }
        Some("--version" | "-V") => println!("opcli {}", env!("CARGO_PKG_VERSION")),
        Some(command) => {
            eprintln!("opcli: unsupported command: {command}");
            std::process::exit(2);
        }
    }
}
