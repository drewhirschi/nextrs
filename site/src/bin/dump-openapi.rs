//! Writes the app's OpenAPI document to `site/client/openapi.json`.
//!
//! This is the bridge between the Rust backend and the generated TypeScript /
//! React Query client: it serializes the same `generated_openapi()` the server
//! serves at `/openapi.json` to a file on disk, so the client can be generated
//! offline without booting the server.
//!
//! Run via the client pipeline (`cd site/client && npm run gen`), or directly:
//!
//! ```sh
//! cargo run -p site --bin dump-openapi
//! ```

include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

fn main() {
    let spec = generated_openapi();
    let json = spec
        .to_pretty_json()
        .expect("serialize OpenAPI document to JSON");

    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/client/openapi.json");
    std::fs::write(out, json).expect("write client/openapi.json");

    eprintln!("wrote {out}");
}
