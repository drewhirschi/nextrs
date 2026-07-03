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
    // Infer `params(...)` from the extractors when the user didn't write it —
    // the handler signature is the single source of truth, so the OpenAPI
    // spec (and the generated client's types) can't silently drift from it.
    if !args_str.contains("params(") && !args_str.contains("params (") {
        if let Ok(func) = syn::parse::<syn::ItemFn>(item.clone()) {
            if let Some(params) = infer_params(&func, &url) {
                injected.push_str(&format!(", params({params})"));
            }
        }
    }
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
/// Eligible: a `Json<...>` or `Result<Json<...>, E>` return type and at most
/// one `Path<...>` plus one `Query<T>` extractor, in any order (including
/// none) — the shapes whose responses the generated client caches under a
/// query key. `Path` values substitute into the URL's `{seg}` slots so the
/// key matches the client's substituted-URL form. Anything else (other
/// extractors — `State`/`Extension` included — opaque `impl IntoResponse`
/// returns, type aliases over Result) gets no companion and routes normally;
/// it just can't be seeded.
///
/// Fallible companions return `Option<SeedEntry>`: an `Err` from the handler
/// seeds nothing, and the page degrades to fetch-on-mount where the hook
/// surfaces the error as usual. Infallible ones return `SeedEntry`;
/// `QuerySeed::seed` accepts both.
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

    // Return type must be Json<...> or Result<Json<...>, E>.
    let syn::ReturnType::Type(_, ret) = &func.sig.output else {
        return None;
    };
    let fallible = match last_path_ident(ret)?.as_str() {
        "Json" => false,
        "Result" => {
            let ok_ty = first_generic_arg(ret)?;
            if last_path_ident(ok_ty)? != "Json" {
                return None;
            }
            true
        }
        _ => return None,
    };

    // Collect the extractors: at most one Path and one Query, in any order.
    // Anything else disqualifies the handler.
    let mut path_ty: Option<&syn::Type> = None;
    let mut query_ty: Option<&syn::Type> = None;
    let mut call_order: Vec<&'static str> = Vec::new();
    for arg in &func.sig.inputs {
        let syn::FnArg::Typed(arg) = arg else {
            return None;
        };
        match last_path_ident(&arg.ty)?.as_str() {
            "Path" if path_ty.is_none() => {
                path_ty = Some(first_generic_arg(&arg.ty)?);
                call_order.push("path");
            }
            "Query" if query_ty.is_none() => {
                query_ty = Some(first_generic_arg(&arg.ty)?);
                call_order.push("query");
            }
            _ => return None,
        }
    }

    // Companion signature: path params first, then the query struct, then the
    // extensions slot — regardless of the handler's declared order (the call
    // below preserves that order).
    let mut sig_args = quote! {};
    // How the seeded entry resolves its URL: with a Path extractor the values
    // substitute into the `{seg}` slots, matching the generated client's key
    // (the *substituted* URL); otherwise the literal URL.
    let url_expr;
    if let Some(pty) = path_ty {
        let seg_count = url.matches('{').count();
        let fmt = url_format_string(url);
        let args: Vec<proc_macro2::TokenStream> = match pty {
            syn::Type::Tuple(tuple) => {
                if tuple.elems.len() != seg_count {
                    return None; // shape mismatch — don't guess
                }
                (0..tuple.elems.len())
                    .map(|i| {
                        let idx = syn::Index::from(i);
                        quote! { &path.#idx }
                    })
                    .collect()
            }
            _ => {
                if seg_count != 1 {
                    return None;
                }
                vec![quote! { &path }]
            }
        };
        sig_args.extend(quote! { path: #pty, });
        url_expr = quote! { format!(#fmt, #(#args),*) };
    } else {
        url_expr = quote! { #url.to_string() };
    }

    let (params_stmt, key_params) = match query_ty {
        Some(qty) => {
            sig_args.extend(quote! { params: #qty, });
            (
                quote! {
                    let __params = ::nextrs::serde_json::to_value(&params)
                        .expect("nextrs seed: params must serialize");
                },
                quote! { Some(__params) },
            )
        }
        None => (quote! {}, quote! { None }),
    };

    let call_args = call_order.iter().map(|which| match *which {
        "path" => quote! { ::nextrs::axum::extract::Path(path) },
        _ => quote! { ::nextrs::axum::extract::Query(params) },
    });

    if fallible {
        // Err seeds nothing — the page falls back to fetch-on-mount and the
        // hook surfaces the error client-side.
        Some(quote! {
            #[doc(hidden)]
            pub async fn __nextrs_seed_get(
                #sig_args
                _ext: &::nextrs::http::Extensions,
            ) -> Option<::nextrs::SeedEntry> {
                let __url = #url_expr;
                #params_stmt
                match #fn_name(#(#call_args),*).await {
                    Ok(__json) => Some(::nextrs::SeedEntry {
                        key: ::nextrs::seed_key(&__url, #key_params),
                        data: ::nextrs::serde_json::to_value(&__json.0)
                            .expect("nextrs seed: response body must serialize"),
                    }),
                    Err(_) => None,
                }
            }
        })
    } else {
        Some(quote! {
            #[doc(hidden)]
            pub async fn __nextrs_seed_get(
                #sig_args
                _ext: &::nextrs::http::Extensions,
            ) -> ::nextrs::SeedEntry {
                let __url = #url_expr;
                #params_stmt
                let __resp = #fn_name(#(#call_args),*).await;
                ::nextrs::SeedEntry {
                    key: ::nextrs::seed_key(&__url, #key_params),
                    data: ::nextrs::serde_json::to_value(&__resp.0)
                        .expect("nextrs seed: response body must serialize"),
                }
            }
        })
    }
}

/// `/api/sources/{id}/pages` → `"/api/sources/{}/pages"` — the `format!`
/// template that substitutes path values into their `{seg}` slots.
fn url_format_string(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    let mut in_seg = false;
    for c in url.chars() {
        match c {
            '{' => {
                in_seg = true;
                out.push_str("{}");
            }
            '}' => in_seg = false,
            _ if in_seg => {}
            _ => out.push(c),
        }
    }
    out
}

/// Derive the utoipa `params(...)` list from the handler's extractors.
///
/// - `Path<T>` args zip with the `{seg}` names from the file-derived URL:
///   scalar `T` ↔ one segment, tuple `(A, B)` ↔ segments in order, a single
///   named struct across several segments → the struct itself (must be
///   `IntoParams`).
/// - `Query<T>` contributes `T` (must be `IntoParams`).
///
/// Returns the inside of `params(...)`, or `None` when there is nothing to
/// declare or the shapes can't be reconciled (then nothing is injected and
/// utoipa/compile errors stay the user's signal, as today).
fn infer_params(func: &syn::ItemFn, url: &str) -> Option<String> {
    use quote::ToTokens;

    // `{id}` → "id", `{*rest}` → "rest", in URL order.
    let path_names: Vec<&str> = url
        .split('/')
        .filter_map(|seg| seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')))
        .map(|name| name.trim_start_matches('*'))
        .collect();

    let mut path_types: Vec<syn::Type> = Vec::new();
    let mut entries: Vec<String> = Vec::new();

    for arg in &func.sig.inputs {
        let syn::FnArg::Typed(arg) = arg else {
            continue;
        };
        match last_path_ident(&arg.ty).as_deref() {
            Some("Path") => {
                let inner = first_generic_arg(&arg.ty)?;
                match inner {
                    syn::Type::Tuple(tuple) => path_types.extend(tuple.elems.iter().cloned()),
                    other => {
                        // One non-tuple, non-primitive type across several URL
                        // segments is a params struct: declare it whole via
                        // IntoParams. A lone primitive there is a shape
                        // mismatch handled below.
                        if path_names.len() > 1 && !is_primitive(other) {
                            entries.push(other.to_token_stream().to_string());
                            continue;
                        }
                        path_types.push(other.clone());
                    }
                }
            }
            Some("Query") => {
                let inner = first_generic_arg(&arg.ty)?;
                entries.push(inner.to_token_stream().to_string());
            }
            _ => {}
        }
    }

    if !path_types.is_empty() {
        if path_types.len() != path_names.len() {
            return None; // shape mismatch — don't guess
        }
        let zipped = path_names.iter().zip(&path_types).map(|(name, ty)| {
            format!("(\"{}\" = {}, Path)", name, ty.to_token_stream())
        });
        // Path params lead, matching their position in the URL.
        entries.splice(0..0, zipped);
    }

    if entries.is_empty() {
        None
    } else {
        Some(entries.join(", "))
    }
}

/// Types that read as a single URL segment value rather than a params struct.
fn is_primitive(ty: &syn::Type) -> bool {
    matches!(
        last_path_ident(ty).as_deref(),
        Some(
            "i8" | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f32"
                | "f64"
                | "bool"
                | "char"
                | "String"
                | "Uuid"
        )
    )
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

    fn parse_fn(src: &str) -> syn::ItemFn {
        syn::parse_str(src).unwrap()
    }

    #[test]
    fn infer_params_scalar_path() {
        let f = parse_fn("pub async fn get(Path(id): Path<i64>) -> Json<X> { todo!() }");
        assert_eq!(
            infer_params(&f, "/api/sources/{id}").as_deref(),
            Some("(\"id\" = i64, Path)")
        );
    }

    #[test]
    fn infer_params_tuple_path() {
        let f =
            parse_fn("pub async fn get(Path((a, b)): Path<(i64, String)>) -> Json<X> { todo!() }");
        assert_eq!(
            infer_params(&f, "/users/{id}/posts/{postId}").as_deref(),
            Some("(\"id\" = i64, Path), (\"postId\" = String, Path)")
        );
    }

    #[test]
    fn infer_params_struct_path_across_segments() {
        let f = parse_fn("pub async fn get(Path(p): Path<PageRef>) -> Json<X> { todo!() }");
        assert_eq!(
            infer_params(&f, "/users/{id}/posts/{postId}").as_deref(),
            Some("PageRef")
        );
    }

    #[test]
    fn infer_params_query() {
        let f = parse_fn("pub async fn get(Query(f): Query<TodosFilter>) -> Json<X> { todo!() }");
        assert_eq!(infer_params(&f, "/api/todos").as_deref(), Some("TodosFilter"));
    }

    #[test]
    fn infer_params_path_and_query() {
        let f = parse_fn(
            "pub async fn get(Path(id): Path<i64>, Query(f): Query<F>) -> Json<X> { todo!() }",
        );
        assert_eq!(
            infer_params(&f, "/api/sources/{id}/pages").as_deref(),
            Some("(\"id\" = i64, Path), F")
        );
    }

    #[test]
    fn infer_params_catch_all_uses_declared_type() {
        let f = parse_fn("pub async fn get(Path(rest): Path<String>) -> Json<X> { todo!() }");
        assert_eq!(
            infer_params(&f, "/files/{*rest}").as_deref(),
            Some("(\"rest\" = String, Path)")
        );
    }

    #[test]
    fn infer_params_nothing_to_declare() {
        let f = parse_fn("pub async fn get() -> Json<X> { todo!() }");
        assert_eq!(infer_params(&f, "/api/ping"), None);
        // Body extractors are not params.
        let f = parse_fn("pub async fn post(Json(b): Json<Req>) -> Json<X> { todo!() }");
        assert_eq!(infer_params(&f, "/api/ping"), None);
    }

    #[test]
    fn infer_params_shape_mismatch_declares_nothing() {
        // Two URL params, a lone scalar Path — don't guess.
        let f = parse_fn("pub async fn get(Path(id): Path<i64>) -> Json<X> { todo!() }");
        assert_eq!(infer_params(&f, "/users/{id}/posts/{postId}"), None);
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
    fn seed_companion_for_scalar_path_get() {
        let item: proc_macro2::TokenStream =
            "pub async fn get(Path(id): Path<i64>) -> Json<Vec<Page>> { todo!() }"
                .parse()
                .unwrap();
        let c = seed_companion(item, "/api/sources/{id}/pages")
            .unwrap()
            .to_string();
        assert!(c.contains("__nextrs_seed_get"), "{}", c);
        assert!(c.contains("path : i64"), "{}", c);
        // Key uses the substituted URL, like the generated client.
        assert!(c.contains(r#""/api/sources/{}/pages""#), "{}", c);
        assert!(c.contains("None"), "{}", c);
    }

    #[test]
    fn seed_companion_for_tuple_path_get() {
        let item: proc_macro2::TokenStream =
            "pub async fn get(Path((a, b)): Path<(i64, i64)>) -> Json<X> { todo!() }"
                .parse()
                .unwrap();
        let c = seed_companion(item, "/api/sources/{id}/regions/{rid}")
            .unwrap()
            .to_string();
        assert!(c.contains(r#""/api/sources/{}/regions/{}""#), "{}", c);
        assert!(c.contains("path . 0"), "{}", c);
        assert!(c.contains("path . 1"), "{}", c);
    }

    #[test]
    fn seed_companion_for_path_and_query_get() {
        let item: proc_macro2::TokenStream =
            "pub async fn get(Path(id): Path<i64>, Query(f): Query<F>) -> Json<X> { todo!() }"
                .parse()
                .unwrap();
        let c = seed_companion(item, "/api/sources/{id}/pages")
            .unwrap()
            .to_string();
        assert!(c.contains("path : i64"), "{}", c);
        assert!(c.contains("params : F"), "{}", c);
        assert!(c.contains("Some (__params)"), "{}", c);
        // The handler call preserves the declared extractor order.
        let path_call = c.find("Path (path)").unwrap();
        let query_call = c.find("Query (params)").unwrap();
        assert!(path_call < query_call, "{}", c);
    }

    #[test]
    fn seed_companion_path_shape_mismatch_is_ineligible() {
        // One scalar Path arg, two URL segments.
        let item: proc_macro2::TokenStream =
            "pub async fn get(Path(id): Path<i64>) -> Json<X> { todo!() }"
                .parse()
                .unwrap();
        assert!(seed_companion(item, "/a/{x}/b/{y}").is_none());
    }

    #[test]
    fn seed_companion_for_fallible_get() {
        let item: proc_macro2::TokenStream =
            "pub async fn get() -> Result<Json<Vec<Todo>>, ApiError> { todo!() }"
                .parse()
                .unwrap();
        let c = seed_companion(item, "/api/todos").unwrap().to_string();
        assert!(c.contains("Option < :: nextrs :: SeedEntry >"), "{}", c);
        assert!(c.contains("Ok (__json)"), "{}", c);
        assert!(c.contains("Err (_) => None"), "{}", c);
    }

    #[test]
    fn seed_companion_for_fallible_path_and_query_get() {
        let item: proc_macro2::TokenStream =
            "pub async fn get(Path(id): Path<i64>, Query(f): Query<F>) -> Result<Json<X>, E> { todo!() }"
                .parse()
                .unwrap();
        let c = seed_companion(item, "/api/sources/{id}/pages")
            .unwrap()
            .to_string();
        assert!(c.contains("path : i64"), "{}", c);
        assert!(c.contains("params : F"), "{}", c);
        assert!(c.contains(r#""/api/sources/{}/pages""#), "{}", c);
        assert!(c.contains("Err (_) => None"), "{}", c);
    }

    #[test]
    fn no_companion_when_result_ok_is_not_json() {
        for src in [
            // Ok side isn't Json.
            "pub async fn get() -> Result<String, E> { todo!() }",
            // Json is in the Err slot only — first generic arg is the Ok side.
            "pub async fn get() -> Result<StatusCode, Json<E>> { todo!() }",
        ] {
            let item: proc_macro2::TokenStream = src.parse().unwrap();
            assert!(seed_companion(item, "/x").is_none(), "{}", src);
        }
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
