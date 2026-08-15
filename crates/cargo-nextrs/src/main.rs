fn main() -> std::process::ExitCode {
    cargo_nextrs::main_with_args("cargo nextrs", std::env::args_os().skip(1))
}
