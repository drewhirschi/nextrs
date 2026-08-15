fn main() {
    eprintln!(
        "warning: `create-nextrs-app` is deprecated; install `cargo-nextrs` and use `nextrs new` or `cargo nextrs new`"
    );
    if let Err(err) = create_nextrs_app::run_with_args(std::env::args().skip(1)) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
