// Writes client/openapi.json — the same spec the app serves at /openapi.json —
// so orval can generate the typed client offline. Run via `npm run gen`.
include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

fn main() {
    let spec = generated_openapi();
    let json = spec.to_pretty_json().expect("serialize OpenAPI document");
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/client/openapi.json");
    std::fs::write(out, json).expect("write client/openapi.json");
    eprintln!("wrote {out}");
}
