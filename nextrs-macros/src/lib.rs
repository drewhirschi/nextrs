//! Proc-macros for nextrs.
//!
//! [`macro@api`] is a thin convenience wrapper around `#[utoipa::path]` that
//! derives the `path = "..."` from the handler's file location, so a typed
//! `route.rs` handler doesn't restate the URL the file convention already
//! encodes.

use proc_macro::{Span, TokenStream};

/// Annotate a `route.rs` method as a typed API endpoint, deriving the OpenAPI
/// `path` from the file's location under `app/`.
///
/// It expands to `#[utoipa::path(...)]` with `path = "..."` filled in, so
/// everything downstream (schema inference, the codegen's spec collection, the
/// generated client) works exactly as if you'd written the `utoipa` attribute
/// by hand — you just don't repeat the path.
///
/// ```ignore
/// // in app/api/ping/route.rs — no `path = "/api/ping"`
/// #[nextrs::api(post, responses((status = 200, body = PingResponse)))]
/// pub async fn post(Json(req): Json<PingRequest>) -> Json<PingResponse> { ... }
/// ```
///
/// All `#[utoipa::path]` arguments pass through. `operation_id` and `tag` are
/// derived from the route when you don't supply them (so the generated hook
/// gets a clean, unique name), and are left alone when you do.
#[proc_macro_attribute]
pub fn api(args: TokenStream, item: TokenStream) -> TokenStream {
    // `Span::call_site()` is the attribute's location; its file is the route.rs.
    // `file()` is relative to the compiling crate's manifest dir, so the same
    // file reads as `app/...` from `site` and `site/app/...` from the deploy
    // crate — `url_from_file` anchors on the `app/` segment to normalize both.
    let url = url_from_file(&Span::call_site().file());
    // A trailing comma is common in the multi-line attribute form; strip it so
    // appending our own arguments doesn't produce a `, ,`.
    let args_str = args.to_string();
    let args_str = args_str.trim().trim_end_matches(',').trim_end();

    let method = args_str
        .split(|c: char| c == ',' || c.is_whitespace())
        .find(|s| !s.is_empty())
        .unwrap_or_default()
        .to_string();

    let mut injected = format!("path = \"{url}\"");
    if !args_str.contains("operation_id") {
        injected.push_str(&format!(
            ", operation_id = \"{}\"",
            default_operation_id(&method, &url)
        ));
    }
    if !args_str.contains("tag =") && !args_str.contains("tag=") {
        if let Some(tag) = default_tag(&url) {
            injected.push_str(&format!(", tag = \"{tag}\""));
        }
    }

    let attr = format!("#[utoipa::path({args_str}, {injected})]");
    let mut out: TokenStream = attr
        .parse()
        .expect("nextrs::api: could not build the utoipa::path attribute");
    out.extend(item);
    out
}

/// Turn a `route.rs` file path into its URL, mirroring `nextrs::discovery`:
/// anchor on the `app/` segment, drop the trailing `route.rs`, and map
/// `[param]` segments to `{param}`.
fn url_from_file(file: &str) -> String {
    let after = file.rsplit_once("app/").map_or(file, |(_, rest)| rest);
    let dir = after
        .strip_suffix("route.rs")
        .unwrap_or(after)
        .trim_end_matches('/');
    if dir.is_empty() {
        return "/".to_string();
    }
    let segments: Vec<String> = dir
        .split('/')
        .map(|seg| match seg.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            Some(param) => format!("{{{param}}}"),
            None => seg.to_string(),
        })
        .collect();
    format!("/{}", segments.join("/"))
}

/// A stable, unique-per-route operation id, e.g. `post` + `/api/ping` →
/// `postApiPing`, `get` + `/users/{id}` → `getUsersById`. Drives the generated
/// hook name (`usePostApiPing`).
fn default_operation_id(method: &str, url: &str) -> String {
    let mut id = method.to_string();
    for seg in url.split('/').filter(|s| !s.is_empty()) {
        match seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            Some(param) => {
                id.push_str("By");
                id.push_str(&pascal(param));
            }
            None => id.push_str(&pascal(seg)),
        }
    }
    id
}

/// Group endpoints by their last static path segment (`/api/users/{id}` →
/// `users`), so the client generator splits files per resource.
fn default_tag(url: &str) -> Option<String> {
    url.split('/')
        .filter(|s| !s.is_empty() && !s.starts_with('{'))
        .next_back()
        .map(str::to_string)
}

fn pascal(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_from_site_relative_path() {
        assert_eq!(url_from_file("app/api/ping/route.rs"), "/api/ping");
    }

    #[test]
    fn url_from_deploy_relative_path() {
        // Same file, seen from the workspace-root crate.
        assert_eq!(url_from_file("site/app/api/ping/route.rs"), "/api/ping");
    }

    #[test]
    fn url_maps_dynamic_segments() {
        assert_eq!(url_from_file("app/users/[id]/route.rs"), "/users/{id}");
        assert_eq!(
            url_from_file("app/users/[id]/posts/[postId]/route.rs"),
            "/users/{id}/posts/{postId}"
        );
    }

    #[test]
    fn url_for_root_route() {
        assert_eq!(url_from_file("app/route.rs"), "/");
    }

    #[test]
    fn operation_ids_are_unique_and_named() {
        assert_eq!(default_operation_id("post", "/api/ping"), "postApiPing");
        assert_eq!(default_operation_id("get", "/users/{id}"), "getUsersById");
    }

    #[test]
    fn tag_is_last_static_segment() {
        assert_eq!(default_tag("/api/ping").as_deref(), Some("ping"));
        assert_eq!(default_tag("/api/users/{id}").as_deref(), Some("users"));
        assert_eq!(default_tag("/"), None);
    }
}
