#![allow(dead_code)]

// Keep the established dev runner implementation in one place while exposing
// it to the unified `cargo-nextrs` command. Inside a library, the included
// `main` is just a private function; the public entry is `run_with_args`.
include!("main.rs");
