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
    out.extend(item.clone());

    // For eligible GET handlers, also emit a typed seed companion so props.rs
    // can warm the React Query cache through the handler (the wire contract),
    // not around it.
    if method == "get" {
        if let Some(companion) = seed_companion(item.into(), &url) {
            out.extend(TokenStream::from(companion));
        }
    }

    out
}

/// Emit `__nextrs_seed_get` next to an eligible GET handler.
///
/// Eligible: zero arguments, or exactly one `Query<T>` extractor, and a
/// `Json<...>` return type — the shapes whose responses the generated client
/// caches under a query key. Anything else (other extractors, other returns)
/// gets no companion and routes normally; it just can't be seeded.
///
/// The companion calls the real handler, so the seeded data is byte-identical
/// to a client refetch. `_ext` is accepted-but-unused: a stable call shape for
/// later middleware-extension forwarding. (It's `&Extensions`, not `&Request`
/// — request bodies aren't `Sync`, and the shell handler's future must be
/// `Send`.)
fn seed_companion(item: proc_macro2::TokenStream, url: &str) -> Option<proc_macro2::TokenStream> {
    use quote::quote;

    let func: syn::ItemFn = syn::parse2(item).ok()?;
    let fn_name = &func.sig.ident;

    // Return type must be Json<...>.
    let syn::ReturnType::Type(_, ret) = &func.sig.output else {
        return None;
    };
    if last_path_ident(ret)? != "Json" {
        return None;
    }

    let inputs: Vec<_> = func.sig.inputs.iter().collect();
    match inputs.as_slice() {
        // pub async fn get() -> Json<T>
        [] => Some(quote! {
            #[doc(hidden)]
            pub async fn __nextrs_seed_get(
                _ext: &::nextrs::http::Extensions,
            ) -> ::nextrs::SeedEntry {
                let __resp = #fn_name().await;
                ::nextrs::SeedEntry {
                    key: ::nextrs::seed_key(#url, None),
                    data: ::nextrs::serde_json::to_value(&__resp.0)
                        .expect("nextrs seed: response body must serialize"),
                }
            }
        }),
        // pub async fn get(Query(f): Query<T>) -> Json<U>
        [syn::FnArg::Typed(arg)] => {
            let query_ty = &arg.ty;
            if last_path_ident(query_ty)? != "Query" {
                return None;
            }
            let params_ty = first_generic_arg(query_ty)?;
            Some(quote! {
                #[doc(hidden)]
                pub async fn __nextrs_seed_get(
                    params: #params_ty,
                    _ext: &::nextrs::http::Extensions,
                ) -> ::nextrs::SeedEntry {
                    let __params = ::nextrs::serde_json::to_value(&params)
                        .expect("nextrs seed: params must serialize");
                    let __resp = #fn_name(::nextrs::axum::extract::Query(params)).await;
                    ::nextrs::SeedEntry {
                        key: ::nextrs::seed_key(#url, Some(__params)),
                        data: ::nextrs::serde_json::to_value(&__resp.0)
                            .expect("nextrs seed: response body must serialize"),
                    }
                }
            })
        }
        _ => None,
    }
}

fn last_path_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => Some(p.path.segments.last()?.ident.to_string()),
        _ => None,
    }
}

fn first_generic_arg(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(p) = ty else { return None };
    let syn::PathArguments::AngleBracketed(args) = &p.path.segments.last()?.arguments else {
        return None;
    };
    args.args.iter().find_map(|a| match a {
        syn::GenericArgument::Type(t) => Some(t),
        _ => None,
    })
}

/// Turn a `route.rs` file path into its URL, mirroring `nextrs::discovery`:
/// anchor on the `app/` segment, drop the trailing `route.rs`, and map
/// `[param]` segments to `{param}` and `[...param]` (catch-all) to `{*param}`.
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
        .map(
            |seg| match seg.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                Some(param) => match param.strip_prefix("...") {
                    Some(rest) => format!("{{*{rest}}}"),
                    None => format!("{{{param}}}"),
                },
                None => seg.to_string(),
            },
        )
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
                id.push_str(&pascal(param.trim_start_matches('*')));
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
    fn url_maps_catch_all_segments() {
        assert_eq!(
            url_from_file("app/api/auth/[...all]/route.rs"),
            "/api/auth/{*all}"
        );
        assert_eq!(
            default_operation_id("post", "/api/auth/{*all}"),
            "postApiAuthByAll"
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

    #[test]
    fn seed_companion_for_query_get() {
        let item: proc_macro2::TokenStream =
            "pub async fn get(Query(f): Query<TodosFilter>) -> Json<Vec<Todo>> { todo!() }"
                .parse()
                .unwrap();
        let c = seed_companion(item, "/api/todos").unwrap().to_string();
        assert!(c.contains("__nextrs_seed_get"), "{}", c);
        assert!(c.contains("TodosFilter"), "{}", c);
        assert!(c.contains("seed_key"), "{}", c);
        assert!(c.contains("\"/api/todos\""), "{}", c);
    }

    #[test]
    fn seed_companion_for_zero_arg_get() {
        let item: proc_macro2::TokenStream = "pub async fn get() -> Json<PingResponse> { todo!() }"
            .parse()
            .unwrap();
        let c = seed_companion(item, "/api/ping").unwrap().to_string();
        assert!(c.contains("__nextrs_seed_get"), "{}", c);
        assert!(c.contains("None"), "{}", c);
    }

    #[test]
    fn no_companion_for_ineligible_shapes() {
        for src in [
            // Non-Json return.
            "pub async fn get() -> impl IntoResponse { todo!() }",
            // Body extractor on a GET.
            "pub async fn get(Json(b): Json<Req>) -> Json<Resp> { todo!() }",
            // Multiple extractors.
            "pub async fn get(Query(f): Query<F>, headers: HeaderMap) -> Json<X> { todo!() }",
        ] {
            let item: proc_macro2::TokenStream = src.parse().unwrap();
            assert!(seed_companion(item, "/x").is_none(), "{}", src);
        }
    }
}
