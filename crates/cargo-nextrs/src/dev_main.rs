fn main() {
    eprintln!("warning: `cargo nextrs-dev` is deprecated; use `cargo nextrs dev` or `nextrs dev`");
    if let Err(error) = cargo_nextrs::dev::run_with_args(std::env::args_os().skip(1)) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
