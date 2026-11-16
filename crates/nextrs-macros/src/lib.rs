//! Proc-macros for nextrs.
//!
//! [`macro@api`] is a thin convenience wrapper around `#[utoipa::path]` that
//! derives the `path = "..."` from the handler's file location, so a typed
//! `route.rs` handler doesn't restate the URL the file convention already
//! encodes.

use proc_macro::{Span, TokenStream};

mod rsx;

/// JSX-shaped HTML templating for Rust server components. Expands to a
/// `String` builder returning [`nextrs::rsx::Rsx`]; static text/attributes are
/// escaped at compile time, `{expr}` holes render via `nextrs::rsx::Render`,
/// and uppercase-first tags call component functions
/// (`<TodoFilter x={1} />` → `TodoFilter(TodoFilterProps { x: 1 })`).
/// See `docs/rsx-server-components.md`.
#[proc_macro]
pub fn rsx(input: TokenStream) -> TokenStream {
    rsx::rsx_impl(input.into()).into()
}

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
/// #[nextrs::api]
/// pub async fn post(Json(req): Json<PingRequest>) -> Json<PingResponse> { ... }
/// ```
///
/// The HTTP method comes from the function name. Request and success response
/// bodies are inferred from `Json<T>`, while the path, `operation_id`, and tag
/// come from the route. All values can still be overridden with ordinary
/// `#[utoipa::path]` arguments when an endpoint has a richer contract.
#[proc_macro_attribute]
pub fn api(args: TokenStream, item: TokenStream) -> TokenStream {
    // `Span::call_site()` is the attribute's location; its file is the route.rs.
    // `file()` is relative to the compiling crate's manifest dir, so the same
    // file reads as `app/...` from `site` and `site/app/...` from the deploy
    // crate — `url_from_file` anchors on the `app/` segment to normalize both.
    let url = url_from_file(&Span::call_site().file());
    // A trailing comma is common in the multi-line attribute form; strip it so
    // appending our own arguments doesn't produce a `, ,`.
    let args_string = args.to_string();
    let args_str = args_string.trim().trim_end_matches(',').trim_end();
    let func = syn::parse::<syn::ItemFn>(item.clone()).ok();
    let fn_method = func
        .as_ref()
        .map(|func| func.sig.ident.to_string().to_lowercase())
        .unwrap_or_default();
    let (method, extra_args) = split_method(args_str, &fn_method);

    if let Some(func) = &func {
        if let Err(error) = validate_path_extractors(func, &url) {
            return error.into_compile_error().into();
        }
    }

    let mut parts = vec![method.clone(), format!("path = \"{url}\"")];
    if !extra_args.is_empty() {
        parts.push(extra_args.to_string());
    }
    // Infer `params(...)` from the extractors when the user didn't write it —
    // the handler signature is the single source of truth, so the OpenAPI
    // spec (and the generated client's types) can't silently drift from it.
    if !extra_args.contains("params(") && !extra_args.contains("params (") {
        if let Some(func) = &func {
            if let Some(params) = infer_params(&func, &url) {
                parts.push(format!("params({params})"));
            }
        }
    }
    if !extra_args.contains("request_body") {
        if let Some(body) = func.as_ref().and_then(infer_request_body) {
            parts.push(format!("request_body = {body}"));
        }
    }
    if !extra_args.contains("responses(") && !extra_args.contains("responses (") {
        // A body-less `StatusCode` handler still gets a 200 so the operation
        // doesn't emit an empty `responses: {}` (which reads as "no contract"
        // downstream). Anything richer needs an explicit block.
        if func.as_ref().is_some_and(|f| returns_bare_status_code(f)) {
            parts.push("responses((status = 200, description = \"\"))".to_string());
        } else if let Some(body) = func.as_ref().and_then(infer_success_body) {
            // `Result<Json<T>, ApiError>` self-registers its error contract: a
            // `default` response with the ApiError schema, so the generated
            // client sees a typed error union with no hand-written block.
            let error = match func.as_ref().and_then(infer_api_error) {
                Some(err) => {
                    format!(", (status = \"default\", description = \"Error\", body = {err})")
                }
                None => String::new(),
            };
            parts.push(format!("responses((status = 200, body = {body}){error})"));
        }
    } else if !declares_success_response(&extra_args) {
        // The user declared error responses but not the 200 the return type
        // already states — merge the inferred success in rather than making
        // one hand-written 404 force restating everything.
        if let Some(body) = func.as_ref().and_then(infer_success_body) {
            if let Some(idx) = parts.iter().position(|p| p.contains("responses")) {
                if let Some(merged) = merge_success_into_responses(&parts[idx], &body) {
                    parts[idx] = merged;
                }
            }
        }
    }
    if !extra_args.contains("operation_id") {
        parts.push(format!(
            "operation_id = \"{}\"",
            default_operation_id(&method, &url)
        ));
    }
    if !extra_args.contains("tag =") && !extra_args.contains("tag=") {
        if let Some(tag) = default_tag(&url) {
            parts.push(format!("tag = \"{tag}\""));
        }
    }

    let attr = format!("#[utoipa::path({})]", parts.join(", "));
    let mut out: TokenStream = attr
        .parse()
        .expect("nextrs::api: could not build the utoipa::path attribute");
    out.extend(item.clone());

    // For eligible GET handlers, also emit a typed seed companion so prefetch.rs
    // can warm the React Query cache through the handler (the wire contract),
    // not around it.
    if method == "get" {
        if let Some(companion) = seed_companion(item.into(), &url) {
            out.extend(TokenStream::from(companion));
        }
    }

    out
}

/// Annotate a `route.rs` handler as a cron target: `#[nextrs::api]` plus the
/// `CRON_SECRET` bearer gate, so a scheduled route cannot forget its auth.
///
/// ```ignore
/// // in app/api/cron/refresh/route.rs
/// #[nextrs::cron(schedule = "0 3 * * *")]
/// pub async fn get(Extension(db): Extension<Db>) -> Result<Json<Report>, StatusCode> {
///     // runs only when the request carries `Authorization: Bearer $CRON_SECRET`
/// }
/// ```
///
/// `schedule` is a required five-field UTC cron expression. `provider` is
/// optional (`"vercel"`, the default, or `"cloudflare"`). Set
/// `disabled = true` to keep the protected route while omitting its trigger.
/// The generated
/// [`nextrs::cron::CronAuth`] extractor rejects unauthorized requests before
/// body-consuming extractors run, so the handler keeps the same return shapes
/// as an ordinary `#[nextrs::api]` handler.
#[proc_macro_attribute]
pub fn cron(args: TokenStream, item: TokenStream) -> TokenStream {
    use quote::quote;
    use syn::parse::Parser as _;

    let parser =
        syn::punctuated::Punctuated::<syn::MetaNameValue, syn::Token![,]>::parse_terminated;
    let args = match parser.parse2(proc_macro2::TokenStream::from(args)) {
        Ok(args) => args,
        Err(error) => return error.into_compile_error().into(),
    };
    let mut schedule = None;
    let mut provider = None;
    let mut disabled = None;
    for arg in args {
        let Some(name) = arg.path.get_ident().map(ToString::to_string) else {
            return syn::Error::new_spanned(
                arg.path,
                "expected `schedule`, `provider`, or `disabled`",
            )
            .into_compile_error()
            .into();
        };
        match name.as_str() {
            "schedule" | "provider" => {
                let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(value),
                    ..
                }) = arg.value
                else {
                    return syn::Error::new_spanned(
                        arg.value,
                        format!("`{name}` must be a string literal"),
                    )
                    .into_compile_error()
                    .into();
                };
                let slot = if name == "schedule" {
                    &mut schedule
                } else {
                    &mut provider
                };
                if slot.replace(value).is_some() {
                    return syn::Error::new_spanned(arg.path, format!("duplicate `{name}`"))
                        .into_compile_error()
                        .into();
                }
            }
            "disabled" => {
                let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Bool(value),
                    ..
                }) = arg.value
                else {
                    return syn::Error::new_spanned(
                        arg.value,
                        "`disabled` must be a boolean literal",
                    )
                    .into_compile_error()
                    .into();
                };
                if disabled.replace(value.value).is_some() {
                    return syn::Error::new_spanned(arg.path, format!("duplicate `{name}`"))
                        .into_compile_error()
                        .into();
                }
            }
            _ => {
                return syn::Error::new_spanned(
                    arg.path,
                    "unknown cron option; expected `schedule`, `provider`, or `disabled`",
                )
                .into_compile_error()
                .into();
            }
        }
    }
    let _disabled = disabled.unwrap_or(false);
    let Some(schedule) = schedule else {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "#[nextrs::cron] requires `schedule = \"...\"`",
        )
        .into_compile_error()
        .into();
    };
    let field_count = schedule.value().split_whitespace().count();
    if field_count != 5 {
        return syn::Error::new_spanned(
            &schedule,
            format!("cron schedule must have 5 fields, found {field_count}"),
        )
        .into_compile_error()
        .into();
    }
    if let Some(provider) = &provider {
        if !matches!(provider.value().as_str(), "vercel" | "cloudflare") {
            return syn::Error::new_spanned(
                provider,
                "cron provider must be `vercel` or `cloudflare`",
            )
            .into_compile_error()
            .into();
        }
    }

    let mut func = match syn::parse::<syn::ItemFn>(item) {
        Ok(func) => func,
        Err(error) => return error.into_compile_error().into(),
    };
    if func.sig.ident != "get" {
        return syn::Error::new_spanned(
            &func.sig,
            "#[nextrs::cron] handlers must be named `get`; Vercel and generated Cloudflare triggers send GET requests",
        )
        .into_compile_error()
        .into();
    }

    // Request-parts extractors must precede a body-consuming final extractor.
    // CronAuth performs the gate during extraction, before Json/Form/body work.
    let arg: syn::FnArg = syn::parse_quote!(_: ::nextrs::cron::CronAuth);
    func.sig.inputs.insert(0, arg);

    quote! {
        #[::nextrs::api]
        #func
    }
    .into()
}

/// Emit `__nextrs_seed_get` next to an eligible GET handler.
///
/// Eligible: a `Json<...>` or `Result<Json<...>, E>` return type and, in any
/// order (including none): at most one `Path<...>`, at most one `Query<T>`,
/// plus any number of `Extension<T>` / `WaitUntil` args — the shapes whose
/// responses the generated client caches under a query key. `Path` values
/// substitute into the URL's `{seg}` slots so the key matches the client's
/// substituted-URL form. Anything else (`State`, opaque `impl IntoResponse`
/// returns, type aliases over Result) gets no companion and routes normally;
/// it just can't be seeded.
///
/// `Extension<T>` and `WaitUntil` are sourced from `_ext` — the request
/// extensions every prefetch call site passes, which already carry
/// layer-installed app state and the Vercel-injected `WaitUntil` (both
/// prefetch paths hand the companion a real, middleware-processed request).
/// They never affect the seed key: it stays URL + query params, matching the
/// client hook.
///
/// Companions return `Option<SeedEntry>` when the handler is fallible OR
/// takes an `Extension`: an `Err` — or a missing extension — seeds nothing,
/// and the page degrades to fetch-on-mount where the hook surfaces the error
/// as usual. Plain infallible ones return `SeedEntry`; `QuerySeed::seed`
/// accepts both. A missing `WaitUntil` never disqualifies — it falls back to
/// the detached `tokio::spawn` form, like the extractor itself.
///
/// The companion calls the real handler, so the seeded data is byte-identical
/// to a client refetch. (`_ext` is `&Extensions`, not `&Request` — request
/// bodies aren't `Sync`, and the shell handler's future must be `Send`.)
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

    // Collect the extractors: at most one Path and one Query (caller-supplied),
    // plus any number of Extension<T> / WaitUntil (sourced from `_ext` — the
    // request extensions every prefetch call site already passes). Anything
    // else disqualifies the handler.
    enum CallArg {
        Path,
        Query,
        Ext(usize),
        Wait,
        Timing,
    }
    let mut path_ty: Option<&syn::Type> = None;
    let mut query_ty: Option<&syn::Type> = None;
    let mut ext_tys: Vec<&syn::Type> = Vec::new();
    let mut call_order: Vec<CallArg> = Vec::new();
    for arg in &func.sig.inputs {
        let syn::FnArg::Typed(arg) = arg else {
            return None;
        };
        match last_path_ident(&arg.ty)?.as_str() {
            "Path" if path_ty.is_none() => {
                path_ty = Some(first_generic_arg(&arg.ty)?);
                call_order.push(CallArg::Path);
            }
            "Query" if query_ty.is_none() => {
                query_ty = Some(first_generic_arg(&arg.ty)?);
                call_order.push(CallArg::Query);
            }
            "Extension" => {
                ext_tys.push(first_generic_arg(&arg.ty)?);
                call_order.push(CallArg::Ext(ext_tys.len() - 1));
            }
            "WaitUntil" => call_order.push(CallArg::Wait),
            "Timing" => call_order.push(CallArg::Timing),
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

    // Extension values come from `_ext`. A missing one seeds nothing (the
    // page degrades to fetch-on-mount, like the fallible-Err path), so any
    // Extension arg makes the companion Option-returning. They never touch
    // the seed key — server context is invisible to the client hook, and the
    // key must stay URL + query params or seeded keys stop matching.
    let ext_stmts: Vec<proc_macro2::TokenStream> = ext_tys
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let ident = quote::format_ident!("__ext{i}");
            quote! {
                let #ident = match _ext.get::<#ty>() {
                    Some(v) => v.clone(),
                    None => return None,
                };
            }
        })
        .collect();

    let call_args = call_order.iter().map(|which| match which {
        CallArg::Path => quote! { ::nextrs::axum::extract::Path(path) },
        CallArg::Query => quote! { ::nextrs::axum::extract::Query(params) },
        CallArg::Ext(i) => {
            let ident = quote::format_ident!("__ext{i}");
            quote! { ::nextrs::axum::Extension(#ident) }
        }
        // Absent extension (local dev, no Vercel layer) falls back to the
        // detached tokio::spawn form — identical to the extractor's behavior.
        CallArg::Wait => quote! {
            _ext.get::<::nextrs::WaitUntil>().cloned().unwrap_or_default()
        },
        // Sourced from the same extensions; during a page render this is the
        // request's telemetry handle, so segments recorded while seeding land
        // in the page's breakdown. No-op when absent — like the extractor.
        CallArg::Timing => quote! {
            ::nextrs::telemetry::Timing::from_extensions(_ext)
        },
    });

    if fallible || !ext_stmts.is_empty() {
        // Err (or a missing extension) seeds nothing — the page falls back to
        // fetch-on-mount and the hook surfaces any error client-side.
        let call = if fallible {
            quote! {
                match #fn_name(#(#call_args),*).await {
                    Ok(__json) => Some(::nextrs::SeedEntry {
                        key: ::nextrs::seed_key(&__url, #key_params),
                        data: ::nextrs::serde_json::to_value(&__json.0)
                            .expect("nextrs seed: response body must serialize"),
                    }),
                    Err(_) => None,
                }
            }
        } else {
            quote! {
                {
                    let __resp = #fn_name(#(#call_args),*).await;
                    Some(::nextrs::SeedEntry {
                        key: ::nextrs::seed_key(&__url, #key_params),
                        data: ::nextrs::serde_json::to_value(&__resp.0)
                            .expect("nextrs seed: response body must serialize"),
                    })
                }
            }
        };
        Some(quote! {
            #[doc(hidden)]
            pub async fn __nextrs_seed_get(
                #sig_args
                _ext: &::nextrs::http::Extensions,
            ) -> Option<::nextrs::SeedEntry> {
                let __url = #url_expr;
                #(#ext_stmts)*
                #params_stmt
                #call
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

/// Annotate the function in an `app/jobs/<name>/job.rs` as a background job.
///
/// The job's stable name is derived from the directory under `app/jobs/`
/// (nested directories join with `/`), the same way [`macro@api`] derives a
/// URL from the file location — the convention encodes the name once.
///
/// ```ignore
/// // in app/jobs/audit-todo/job.rs
/// // all optional; defaults: 5 attempts, 60s timeout, 30s back-off doubling to 1h
/// #[nextrs::job(max_attempts = 5, timeout_secs = 120, backoff_secs = 2, max_backoff_secs = 60)]
/// pub async fn audit_todo(
///     Extension(ctx): Extension<TodosCtx>,   // 0..n app-state extensions
///     payload: AuditTodo,                    // at most one payload arg
/// ) -> Result<Audited, anyhow::Error> { ... } // or `-> ()`; `Audited: Serialize`
/// ```
///
/// Calling `audit_todo(payload)` from app code does **not** run the body: the
/// macro renames the body away and re-emits the original name as a typed
/// enqueue wrapper that persists a job row and POSTs the job's own route
/// (`/__nx/jobs/<name>`) on this deployment. The body runs inside *that*
/// request, behind the framework-managed `WaitUntil` — user code never touches
/// `WaitUntil`, timeouts, or retries. `Err`, panic, or timeout mark the row
/// failed and it retries with exponential back-off (`backoff_secs · 2^(n-1)`,
/// capped at `max_backoff_secs`) up to `max_attempts`. An `Ok` value is
/// stored as the row's `result`; every attempt (duration, error, captured
/// log lines) is kept in its `history`. Inside the body,
/// `nextrs::jobs::current()` says which attempt is running.
///
/// Emitted alongside the wrapper (all `#[doc(hidden)]`, consumed by the
/// build-time codegen): `__nextrs_job_run` (JSON-payload runner the framework
/// route calls), `__NEXTRS_JOB_NAME`, `__NEXTRS_JOB_TIMEOUT_MS`,
/// `__NEXTRS_JOB_MAX_ATTEMPTS`, `__NEXTRS_JOB_BACKOFF_MS`,
/// `__NEXTRS_JOB_MAX_BACKOFF_MS`.
#[proc_macro_attribute]
pub fn job(args: TokenStream, item: TokenStream) -> TokenStream {
    let file = Span::call_site().file();
    let Some(name) = job_name_from_file(&file) else {
        let msg = format!(
            "#[nextrs::job] functions must live in app/jobs/<name>/job.rs \
             (this file is `{file}`)"
        );
        let mut out: TokenStream = format!("::core::compile_error!({msg:?});")
            .parse()
            .expect("nextrs::job: could not build compile_error");
        out.extend(item);
        return out;
    };
    match job_expand(args.into(), item.clone().into(), &name) {
        Ok(ts) => ts.into(),
        Err(e) => {
            // Emit the error alongside the original item so downstream errors
            // don't cascade on top of the real one.
            let mut out = TokenStream::from(e.to_compile_error());
            out.extend(item);
            out
        }
    }
}

/// `app/jobs/audit-todo/job.rs` → `audit-todo`;
/// `app/jobs/email/welcome/job.rs` → `email/welcome`.
/// Anchors on the `app/jobs/` segment (crate-relative or workspace-relative
/// paths both work, like `url_from_file`). `None` when the file isn't a
/// `job.rs` under `app/jobs/` with at least one directory of name.
fn job_name_from_file(file: &str) -> Option<String> {
    let (_, rest) = file.rsplit_once("app/jobs/")?;
    let name = rest.strip_suffix("job.rs")?.trim_end_matches('/');
    if name.is_empty() || name.split('/').any(|seg| seg.is_empty()) {
        return None;
    }
    Some(name.to_string())
}

/// The full `#[nextrs::job]` expansion. Separated from the attribute entry
/// point so tests can drive it with an explicit name (spans have no file in
/// unit tests).
fn job_expand(
    args: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
    name: &str,
) -> syn::Result<proc_macro2::TokenStream> {
    use quote::quote;
    use syn::parse::Parser;

    // ---- macro args, all optional: `max_attempts = N`, `timeout_secs = N`,
    // `backoff_secs = N` (first retry delay, doubling), `max_backoff_secs = N`.
    let mut max_attempts: u32 = 5;
    let mut timeout_ms: u64 = 60_000;
    let mut backoff_ms: u64 = 30_000;
    let mut max_backoff_ms: u64 = 3_600_000;
    let parsed =
        syn::punctuated::Punctuated::<syn::MetaNameValue, syn::Token![,]>::parse_terminated
            .parse2(args)?;
    for nv in &parsed {
        let key = nv
            .path
            .get_ident()
            .map(|i| i.to_string())
            .unwrap_or_default();
        let lit_int = match &nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(i),
                ..
            }) => i,
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "#[nextrs::job]: expected an integer literal",
                ))
            }
        };
        match key.as_str() {
            "max_attempts" => max_attempts = lit_int.base10_parse()?,
            "timeout_secs" => timeout_ms = lit_int.base10_parse::<u64>()?.saturating_mul(1000),
            "backoff_secs" => backoff_ms = lit_int.base10_parse::<u64>()?.saturating_mul(1000),
            "max_backoff_secs" => {
                max_backoff_ms = lit_int.base10_parse::<u64>()?.saturating_mul(1000)
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    &nv.path,
                    "#[nextrs::job]: unknown argument (expected `max_attempts`, `timeout_secs`, \
                     `backoff_secs`, or `max_backoff_secs`)",
                ))
            }
        }
    }

    // ---- the function shape.
    let func: syn::ItemFn = syn::parse2(item)?;
    if func.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            &func.sig.fn_token,
            "#[nextrs::job]: job functions must be `async`",
        ));
    }
    if !matches!(func.vis, syn::Visibility::Public(_)) {
        return Err(syn::Error::new_spanned(
            &func.sig.ident,
            "#[nextrs::job]: job functions must be `pub` (the enqueue wrapper takes the name)",
        ));
    }

    enum CallArg {
        Ext(usize),
        Payload,
    }
    let mut ext_tys: Vec<&syn::Type> = Vec::new();
    let mut payload_ty: Option<&syn::Type> = None;
    let mut call_order: Vec<CallArg> = Vec::new();
    for arg in &func.sig.inputs {
        let syn::FnArg::Typed(arg) = arg else {
            return Err(syn::Error::new_spanned(
                arg,
                "#[nextrs::job]: job functions take no receiver",
            ));
        };
        match last_path_ident(&arg.ty).as_deref() {
            Some("Extension") => {
                let inner = first_generic_arg(&arg.ty).ok_or_else(|| {
                    syn::Error::new_spanned(&arg.ty, "#[nextrs::job]: Extension needs a type argument")
                })?;
                ext_tys.push(inner);
                call_order.push(CallArg::Ext(ext_tys.len() - 1));
            }
            Some("WaitUntil") => {
                return Err(syn::Error::new_spanned(
                    &arg.ty,
                    "#[nextrs::job]: jobs already run behind WaitUntil — remove the extractor; \
                     the framework schedules, times out, and retries the body for you",
                ))
            }
            Some("State" | "Path" | "Query" | "Json" | "HeaderMap" | "Timing" | "Request") => {
                return Err(syn::Error::new_spanned(
                    &arg.ty,
                    "#[nextrs::job]: jobs accept only Extension<T> args plus one plain \
                     Serialize + DeserializeOwned payload argument",
                ))
            }
            _ => {
                if payload_ty.is_some() {
                    return Err(syn::Error::new_spanned(
                        &arg.ty,
                        "#[nextrs::job]: at most one payload argument \
                         (bundle multiple values into one struct)",
                    ));
                }
                payload_ty = Some(&arg.ty);
                call_order.push(CallArg::Payload);
            }
        }
    }

    let fallible = match &func.sig.output {
        syn::ReturnType::Default => false,
        syn::ReturnType::Type(_, ty) => match &**ty {
            syn::Type::Tuple(t) if t.elems.is_empty() => false,
            other => match last_path_ident(other).as_deref() {
                Some("Result") => true,
                _ => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "#[nextrs::job]: job functions return `()` or `Result<T: Serialize, E: Display>` \
                         — the result lives in the job row (and the dashboard), not the call site",
                    ))
                }
            },
        },
    };

    // ---- emission.
    // (1) The user's body, renamed. Only `__nextrs_job_run` calls it.
    let mut impl_fn = func.clone();
    let orig_ident = func.sig.ident.clone();
    impl_fn.sig.ident = syn::Ident::new("__nextrs_job_impl", orig_ident.span());
    impl_fn.attrs.push(syn::parse_quote!(#[doc(hidden)]));

    // (2) The runner: JSON in, typed call, Display-stringified error out.
    let payload_stmt = match payload_ty {
        Some(ty) => quote! {
            let __payload: #ty = ::nextrs::serde_json::from_value(__payload_json)
                .map_err(|e| ::std::format!("payload deserialize: {e}"))?;
        },
        None => quote! { let _ = __payload_json; },
    };
    let ext_stmts: Vec<proc_macro2::TokenStream> = ext_tys
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let ident = quote::format_ident!("__ext{i}");
            let ty_str = quote!(#ty).to_string();
            quote! {
                let #ident = match _ext.get::<#ty>() {
                    Some(v) => v.clone(),
                    None => return Err(::std::format!("missing extension {}", #ty_str)),
                };
            }
        })
        .collect();
    let call_args = call_order.iter().map(|which| match which {
        CallArg::Ext(i) => {
            let ident = quote::format_ident!("__ext{i}");
            quote! { ::nextrs::axum::Extension(#ident) }
        }
        CallArg::Payload => quote! { __payload },
    });
    let call = if fallible {
        quote! {
            match __nextrs_job_impl(#(#call_args),*).await {
                Ok(v) => ::nextrs::serde_json::to_value(&v)
                    .map_err(|e| ::std::format!("return value did not serialize: {e}")),
                Err(e) => Err(::std::string::ToString::to_string(&e)),
            }
        }
    } else {
        quote! {
            __nextrs_job_impl(#(#call_args),*).await;
            Ok(::nextrs::serde_json::Value::Null)
        }
    };

    // (3) The enqueue wrapper under the original name — what app code calls.
    let (wrapper_sig, payload_value) = match payload_ty {
        Some(ty) => (
            quote! { payload: #ty },
            quote! { ::nextrs::serde_json::to_value(&payload)? },
        ),
        None => (quote! {}, quote! { ::nextrs::serde_json::Value::Null }),
    };
    let wrapper_doc = format!(
        "Enqueue the `{name}` background job.\n\n\
         Persists a job row and kicks off `POST /__nx/jobs/{name}` on this \
         deployment; the job body runs there, behind the framework-managed \
         `WaitUntil`, with retries and a {timeout_ms} ms timeout. Returns \
         immediately with a [`JobHandle`](::nextrs::jobs::JobHandle)."
    );

    Ok(quote! {
        #impl_fn

        #[doc(hidden)]
        pub async fn __nextrs_job_run(
            __payload_json: ::nextrs::serde_json::Value,
            _ext: &::nextrs::http::Extensions,
        ) -> ::core::result::Result<::nextrs::serde_json::Value, ::std::string::String> {
            #payload_stmt
            #(#ext_stmts)*
            #call
        }

        #[doc(hidden)]
        pub const __NEXTRS_JOB_NAME: &::core::primitive::str = #name;
        #[doc(hidden)]
        pub const __NEXTRS_JOB_TIMEOUT_MS: ::core::primitive::u64 = #timeout_ms;
        #[doc(hidden)]
        pub const __NEXTRS_JOB_MAX_ATTEMPTS: ::core::primitive::u32 = #max_attempts;
        #[doc(hidden)]
        pub const __NEXTRS_JOB_BACKOFF_MS: ::core::primitive::u64 = #backoff_ms;
        #[doc(hidden)]
        pub const __NEXTRS_JOB_MAX_BACKOFF_MS: ::core::primitive::u64 = #max_backoff_ms;

        #[doc = #wrapper_doc]
        pub async fn #orig_ident(
            #wrapper_sig
        ) -> ::core::result::Result<::nextrs::jobs::JobHandle, ::nextrs::jobs::EnqueueError> {
            ::nextrs::jobs::enqueue(
                __NEXTRS_JOB_NAME,
                #payload_value,
                __NEXTRS_JOB_MAX_ATTEMPTS,
            )
            .await
        }
    })
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
        let zipped = path_names
            .iter()
            .zip(&path_types)
            .map(|(name, ty)| format!("(\"{}\" = {}, Path)", name, ty.to_token_stream()));
        // Path params lead, matching their position in the URL.
        entries.splice(0..0, zipped);
    }

    if entries.is_empty() {
        None
    } else {
        Some(entries.join(", "))
    }
}

/// Reject path extractor shapes that cannot deserialize the file-derived URL.
///
/// A named `Path<MyParams>` remains valid for any non-zero segment count: a
/// proc macro cannot resolve `MyParams` to inspect its fields, and Axum/Serde
/// handle that shape. A missing `Path` is also valid because handlers may
/// intentionally ignore dynamic segments.
fn validate_path_extractors(func: &syn::ItemFn, url: &str) -> syn::Result<()> {
    use quote::ToTokens;
    use syn::spanned::Spanned;

    let path_names: Vec<&str> = url
        .split('/')
        .filter_map(|segment| {
            segment
                .strip_prefix('{')
                .and_then(|name| name.strip_suffix('}'))
        })
        .map(|name| name.trim_start_matches('*'))
        .collect();
    let path_args: Vec<&syn::PatType> = func
        .sig
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            syn::FnArg::Typed(arg) if last_path_ident(&arg.ty).as_deref() == Some("Path") => {
                Some(arg)
            }
            _ => None,
        })
        .collect();

    if path_args.len() > 1 {
        return Err(syn::Error::new(
            path_args[1].ty.span(),
            format!(
                "nextrs route `{url}` uses {} separate `Path` extractors; combine them into one `Path<(T1, T2)>` or one named `Path<YourPathStruct>`",
                path_args.len()
            ),
        ));
    }
    let Some(path_arg) = path_args.first() else {
        return Ok(());
    };
    let inner = first_generic_arg(&path_arg.ty).ok_or_else(|| {
        syn::Error::new(
            path_arg.ty.span(),
            "`Path` must have an inner type, such as `Path<u64>` or `Path<MyPath>`",
        )
    })?;

    if path_names.is_empty() {
        return Err(syn::Error::new(
            path_arg.ty.span(),
            format!(
                "nextrs route `{url}` has no dynamic path parameters, so `{}` cannot extract a path value",
                path_arg.ty.to_token_stream()
            ),
        ));
    }

    if let syn::Type::Tuple(tuple) = inner {
        if tuple.elems.len() != path_names.len() {
            return Err(syn::Error::new(
                inner.span(),
                format!(
                    "nextrs route `{url}` has {} dynamic path parameters ({}) but this `Path` tuple has {} elements; use one tuple element per segment or a named `Path<YourPathStruct>`",
                    path_names.len(),
                    path_names.join(", "),
                    tuple.elems.len()
                ),
            ));
        }
    } else if is_primitive(inner) && path_names.len() != 1 {
        return Err(syn::Error::new(
            inner.span(),
            format!(
                "nextrs route `{url}` has {} dynamic path parameters ({}) but `Path<{}>` extracts one scalar value; use `Path<(T1, T2)>` or a named `Path<YourPathStruct>`",
                path_names.len(),
                path_names.join(", "),
                inner.to_token_stream()
            ),
        ));
    }

    Ok(())
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

fn second_generic_arg(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(p) = ty else { return None };
    let syn::PathArguments::AngleBracketed(args) = &p.path.segments.last()?.arguments else {
        return None;
    };
    args.args
        .iter()
        .filter_map(|a| match a {
            syn::GenericArgument::Type(t) => Some(t),
            _ => None,
        })
        .nth(1)
}

/// Accept the old `#[nextrs::api(get, ...)]` spelling, but make the handler
/// name the default source of truth for the method.
fn split_method<'a>(args: &'a str, fn_method: &str) -> (String, &'a str) {
    const METHODS: &[&str] = &[
        "get", "post", "put", "delete", "patch", "head", "options", "trace",
    ];
    let first_end = args
        .find(|c: char| c == ',' || c.is_whitespace())
        .unwrap_or(args.len());
    let first = &args[..first_end];
    if METHODS.contains(&first) {
        let rest = args[first_end..]
            .trim_start()
            .strip_prefix(',')
            .unwrap_or(&args[first_end..])
            .trim_start();
        (first.to_string(), rest)
    } else {
        (fn_method.to_string(), args)
    }
}

fn infer_request_body(func: &syn::ItemFn) -> Option<String> {
    use quote::ToTokens;

    func.sig.inputs.iter().find_map(|arg| {
        let syn::FnArg::Typed(arg) = arg else {
            return None;
        };
        (last_path_ident(&arg.ty).as_deref() == Some("Json"))
            .then(|| first_generic_arg(&arg.ty))
            .flatten()
            .map(|ty| ty.to_token_stream().to_string())
    })
}

fn infer_success_body(func: &syn::ItemFn) -> Option<String> {
    use quote::ToTokens;

    let syn::ReturnType::Type(_, ret) = &func.sig.output else {
        return None;
    };
    let response = match last_path_ident(ret)?.as_str() {
        "Json" => first_generic_arg(ret)?,
        "Result" => {
            let ok = first_generic_arg(ret)?;
            if last_path_ident(ok)?.as_str() != "Json" {
                return None;
            }
            first_generic_arg(ok)?
        }
        _ => return None,
    };
    Some(response.to_token_stream().to_string())
}

/// A handler returning `StatusCode` outright — effect-only, no body.
fn returns_bare_status_code(func: &syn::ItemFn) -> bool {
    let syn::ReturnType::Type(_, ret) = &func.sig.output else {
        return false;
    };
    last_path_ident(ret).as_deref() == Some("StatusCode")
}

/// `Result<Json<T>, E>` where `E`'s last path segment is `ApiError` — the
/// framework error convention. Returns `E`'s tokens (as written, so a path
/// qualifier like `nextrs::ApiError` survives) for use as the error body type.
fn infer_api_error(func: &syn::ItemFn) -> Option<String> {
    use quote::ToTokens;

    let syn::ReturnType::Type(_, ret) = &func.sig.output else {
        return None;
    };
    if last_path_ident(ret)?.as_str() != "Result" {
        return None;
    }
    let err = second_generic_arg(ret)?;
    (last_path_ident(err)?.as_str() == "ApiError").then(|| err.to_token_stream().to_string())
}

/// Does the user's `responses(...)` already declare a success (2xx) response?
/// Textual, like the rest of the attribute handling: whitespace-insensitive
/// match on `status = 2xx` / `status = OK` / `status = StatusCode::OK`.
fn declares_success_response(extra_args: &str) -> bool {
    let compact: String = extra_args.chars().filter(|c| !c.is_whitespace()).collect();
    compact.contains("status=2") || compact.contains("status=OK") || compact.contains("::OK")
}

/// Insert `(status = 200, body = {body})` as the first entry of the
/// `responses(...)` group inside `args`. Returns `None` if no group is found
/// (caller keeps the original).
fn merge_success_into_responses(args: &str, body: &str) -> Option<String> {
    let start = args.find("responses")?;
    let open = start + args[start..].find('(')?;
    let mut out = String::with_capacity(args.len() + body.len() + 32);
    out.push_str(&args[..=open]);
    out.push_str(&format!("(status = 200, body = {body}), "));
    out.push_str(&args[open + 1..]);
    Some(out)
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
    fn method_defaults_to_function_name_and_accepts_legacy_argument() {
        assert_eq!(split_method("", "get"), ("get".into(), ""));
        assert_eq!(
            split_method("operation_id = \"sendPing\"", "post"),
            ("post".into(), "operation_id = \"sendPing\"")
        );
        assert_eq!(
            split_method("post, operation_id = \"sendPing\"", "ignored"),
            ("post".into(), "operation_id = \"sendPing\"")
        );
    }

    #[test]
    fn infers_json_request_and_response_types() {
        let f = parse_fn(
            "pub async fn post(Json(body): Json<CreateGreeting>) -> Json<Greeting> { todo!() }",
        );
        assert_eq!(infer_request_body(&f).as_deref(), Some("CreateGreeting"));
        assert_eq!(infer_success_body(&f).as_deref(), Some("Greeting"));

        let fallible =
            parse_fn("pub async fn get() -> Result<Json<Vec<Greeting>>, AppError> { todo!() }");
        assert_eq!(
            infer_success_body(&fallible).as_deref(),
            Some("Vec < Greeting >")
        );
    }

    #[test]
    fn api_error_returns_are_recognized_structurally() {
        let f = parse_fn("pub async fn get() -> Result<Json<Todo>, ApiError> { todo!() }");
        assert_eq!(infer_api_error(&f).as_deref(), Some("ApiError"));

        // Path qualifiers survive as written.
        let f = parse_fn("pub async fn get() -> Result<Json<Todo>, nextrs::ApiError> { todo!() }");
        assert_eq!(infer_api_error(&f).as_deref(), Some("nextrs :: ApiError"));

        // Other error types don't self-register a schema they may not have.
        let f = parse_fn("pub async fn get() -> Result<Json<Todo>, StatusCode> { todo!() }");
        assert_eq!(infer_api_error(&f), None);
        let f = parse_fn("pub async fn get() -> Json<Todo> { todo!() }");
        assert_eq!(infer_api_error(&f), None);
    }

    #[test]
    fn success_response_detection_is_whitespace_insensitive() {
        assert!(declares_success_response(
            "responses((status = 200, body = T))"
        ));
        assert!(declares_success_response("responses((status=201))"));
        assert!(declares_success_response(
            "responses((status = StatusCode::OK))"
        ));
        assert!(!declares_success_response(
            "responses((status = 404, description = \"missing\"))"
        ));
    }

    #[test]
    fn inferred_success_merges_into_declared_responses() {
        assert_eq!(
            merge_success_into_responses(
                "operation_id = \"getTodo\", responses((status = 404, body = ApiError))",
                "TodoDetail",
            )
            .as_deref(),
            Some(
                "operation_id = \"getTodo\", responses((status = 200, body = TodoDetail), (status = 404, body = ApiError))"
            )
        );
        assert_eq!(
            merge_success_into_responses("operation_id = \"x\"", "T"),
            None
        );
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
        assert_eq!(
            infer_params(&f, "/api/todos").as_deref(),
            Some("TodosFilter")
        );
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
    fn path_validation_accepts_scalar_tuple_struct_and_ignored_params() {
        for (source, url) in [
            (
                "pub async fn get(Path(id): Path<u64>) -> Json<X> { todo!() }",
                "/users/{id}",
            ),
            (
                "pub async fn get(Path(ids): Path<(u64, String)>) -> Json<X> { todo!() }",
                "/users/{user_id}/posts/{post_id}",
            ),
            (
                "pub async fn get(Path(path): Path<PostPath>) -> Json<X> { todo!() }",
                "/users/{user_id}/posts/{post_id}",
            ),
            (
                "pub async fn get() -> Json<X> { todo!() }",
                "/users/{user_id}/posts/{post_id}",
            ),
        ] {
            assert!(
                validate_path_extractors(&parse_fn(source), url).is_ok(),
                "{source} at {url}"
            );
        }
    }

    #[test]
    fn path_validation_rejects_definitely_invalid_shapes() {
        let cases = [
            (
                "pub async fn get(Path(id): Path<u64>) -> Json<X> { todo!() }",
                "/users/{user_id}/posts/{post_id}",
                "has 2 dynamic path parameters (user_id, post_id)",
            ),
            (
                "pub async fn get(Path(ids): Path<(u64, u64, u64)>) -> Json<X> { todo!() }",
                "/users/{user_id}/posts/{post_id}",
                "tuple has 3 elements",
            ),
            (
                "pub async fn get(Path(a): Path<u64>, Path(b): Path<u64>) -> Json<X> { todo!() }",
                "/users/{user_id}/posts/{post_id}",
                "2 separate `Path` extractors",
            ),
            (
                "pub async fn get(Path(id): Path<u64>) -> Json<X> { todo!() }",
                "/users",
                "has no dynamic path parameters",
            ),
        ];

        for (source, url, expected) in cases {
            let error = validate_path_extractors(&parse_fn(source), url).unwrap_err();
            assert!(
                error.to_string().contains(expected),
                "expected {expected:?}, got {error}"
            );
            assert!(
                error.to_string().contains("Path"),
                "diagnostic should explain the Path fix: {error}"
            );
        }
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
            // Unknown extractor.
            "pub async fn get(Query(f): Query<F>, headers: HeaderMap) -> Json<X> { todo!() }",
            // State stays out — registry routing has no Router::with_state.
            "pub async fn get(State(db): State<Db>) -> Json<X> { todo!() }",
        ] {
            let item: proc_macro2::TokenStream = src.parse().unwrap();
            assert!(seed_companion(item, "/x").is_none(), "{}", src);
        }
    }

    #[test]
    fn seed_companion_for_extension_get_sources_ext_and_is_optional() {
        let item: proc_macro2::TokenStream =
            "pub async fn get(Extension(ctx): Extension<AppCtx>, Query(f): Query<F>) -> Json<X> { todo!() }"
                .parse()
                .unwrap();
        let c = seed_companion(item, "/api/status").unwrap().to_string();
        // Sourced from _ext, not the companion signature…
        assert!(c.contains("_ext . get :: < AppCtx >"), "{}", c);
        assert!(!c.contains("ctx : AppCtx"), "{}", c);
        // …missing extension seeds nothing (Option-returning even though the
        // handler is infallible)…
        assert!(c.contains("Option < :: nextrs :: SeedEntry >"), "{}", c);
        assert!(c.contains("None => return None"), "{}", c);
        // …and the declared arg order is preserved in the call.
        let ext_call = c.find("Extension (__ext0)").unwrap();
        let query_call = c.find("Query (params)").unwrap();
        assert!(ext_call < query_call, "{}", c);
        // The key ignores the extension: still URL + query params only.
        assert!(c.contains("Some (__params)"), "{}", c);
    }

    #[test]
    fn seed_companion_for_wait_until_get_stays_infallible() {
        let item: proc_macro2::TokenStream =
            "pub async fn get(wait: WaitUntil) -> Json<X> { todo!() }"
                .parse()
                .unwrap();
        let c = seed_companion(item, "/api/x").unwrap().to_string();
        // No Extension arg → still the plain SeedEntry-returning shape…
        assert!(!c.contains("Option < :: nextrs :: SeedEntry >"), "{}", c);
        // …with the WaitUntil pulled from _ext, detached when absent.
        assert!(
            c.contains("WaitUntil > () . cloned () . unwrap_or_default ()"),
            "{}",
            c
        );
    }

    #[test]
    fn seed_companion_for_timing_get_stays_infallible() {
        let item: proc_macro2::TokenStream =
            "pub async fn get(timing: nextrs::Timing, Query(f): Query<F>) -> Json<X> { todo!() }"
                .parse()
                .unwrap();
        let c = seed_companion(item, "/api/x").unwrap().to_string();
        // Timing never fails → still the plain SeedEntry-returning shape…
        assert!(!c.contains("Option < :: nextrs :: SeedEntry >"), "{}", c);
        // …with the Timing rebuilt from _ext (no-op when absent).
        assert!(c.contains("Timing :: from_extensions (_ext)"), "{}", c);
    }

    #[test]
    fn job_name_from_expected_paths() {
        assert_eq!(
            job_name_from_file("app/jobs/audit-todo/job.rs").as_deref(),
            Some("audit-todo")
        );
        // Workspace-relative view of the same file.
        assert_eq!(
            job_name_from_file("examples/react-todos/app/jobs/audit-todo/job.rs").as_deref(),
            Some("audit-todo")
        );
        // Nested names join with `/`.
        assert_eq!(
            job_name_from_file("app/jobs/email/welcome/job.rs").as_deref(),
            Some("email/welcome")
        );
    }

    #[test]
    fn job_name_rejects_bad_paths() {
        // job.rs directly under app/jobs/ has no name.
        assert_eq!(job_name_from_file("app/jobs/job.rs"), None);
        // Not under app/jobs/ at all.
        assert_eq!(job_name_from_file("app/api/ping/route.rs"), None);
        assert_eq!(job_name_from_file("src/main.rs"), None);
    }

    fn expand_job(args: &str, src: &str) -> Result<String, syn::Error> {
        job_expand(args.parse().unwrap(), src.parse().unwrap(), "audit-todo")
            .map(|ts| ts.to_string())
    }

    #[test]
    fn job_emits_all_four_pieces() {
        let c = expand_job(
            "",
            "pub async fn audit_todo(payload: AuditTodo) -> Result<(), anyhow::Error> { todo!() }",
        )
        .unwrap();
        // (1) body renamed, (2) runner, (3) consts, (4) wrapper under the name.
        assert!(c.contains("async fn __nextrs_job_impl"), "{}", c);
        assert!(c.contains("__nextrs_job_run"), "{}", c);
        assert!(c.contains("__NEXTRS_JOB_NAME : & :: core :: primitive :: str = \"audit-todo\""), "{}", c);
        assert!(c.contains("__NEXTRS_JOB_TIMEOUT_MS : :: core :: primitive :: u64 = 60000u64"), "{}", c);
        assert!(c.contains("__NEXTRS_JOB_MAX_ATTEMPTS : :: core :: primitive :: u32 = 5u32"), "{}", c);
        assert!(c.contains("pub async fn audit_todo (payload : AuditTodo)"), "{}", c);
        assert!(c.contains("jobs :: enqueue"), "{}", c);
        assert!(c.contains("JobHandle"), "{}", c);
        // Fallible body: error stringified via Display into the job row.
        assert!(c.contains("ToString :: to_string"), "{}", c);
    }

    #[test]
    fn job_args_override_consts() {
        let c = expand_job(
            "max_attempts = 3, timeout_secs = 120",
            "pub async fn audit_todo(payload: A) { }",
        )
        .unwrap();
        assert!(c.contains("__NEXTRS_JOB_TIMEOUT_MS : :: core :: primitive :: u64 = 120000u64"), "{}", c);
        assert!(c.contains("__NEXTRS_JOB_MAX_ATTEMPTS : :: core :: primitive :: u32 = 3u32"), "{}", c);
    }

    #[test]
    fn job_extension_args_come_from_request_extensions() {
        let c = expand_job(
            "",
            "pub async fn audit_todo(Extension(ctx): Extension<TodosCtx>, payload: A) -> Result<(), E> { todo!() }",
        )
        .unwrap();
        // Extension sourced from _ext at run time, missing → Err (retryable)…
        assert!(c.contains("_ext . get :: < TodosCtx >"), "{}", c);
        assert!(c.contains("missing extension"), "{}", c);
        // …and the wrapper takes ONLY the payload — state never crosses enqueue.
        assert!(c.contains("pub async fn audit_todo (payload : A)"), "{}", c);
        // Declared arg order preserved in the impl call.
        let ext_call = c.find("Extension (__ext0)").unwrap();
        let payload_call = c.find("__ext0) , __payload").map(|_| ()).is_some();
        assert!(payload_call, "{}", c);
        let _ = ext_call;
    }

    #[test]
    fn job_without_payload_enqueues_null() {
        let c = expand_job("", "pub async fn audit_todo() { }").unwrap();
        assert!(c.contains("pub async fn audit_todo ()"), "{}", c);
        assert!(c.contains("Value :: Null"), "{}", c);
        // Runner ignores the JSON payload.
        assert!(c.contains("let _ = __payload_json"), "{}", c);
    }

    #[test]
    fn job_rejects_ineligible_shapes() {
        for (args, src, needle) in [
            ("", "pub fn audit_todo(p: A) { }", "must be `async`"),
            ("", "async fn audit_todo(p: A) { }", "must be `pub`"),
            ("", "pub async fn audit_todo(w: WaitUntil, p: A) { }", "already run behind WaitUntil"),
            ("", "pub async fn audit_todo(State(db): State<Db>, p: A) { }", "only Extension"),
            ("", "pub async fn audit_todo(Json(b): Json<A>) { }", "only Extension"),
            ("", "pub async fn audit_todo(a: A, b: B) { }", "at most one payload"),
            ("", "pub async fn audit_todo(p: A) -> Json<X> { todo!() }", "return `()` or `Result"),
            ("bogus = 1", "pub async fn audit_todo(p: A) { }", "unknown argument"),
            ("max_attempts = \"x\"", "pub async fn audit_todo(p: A) { }", "integer literal"),
        ] {
            let err = expand_job(args, src).unwrap_err().to_string();
            assert!(err.contains(needle), "src={src} err={err}");
        }
    }

    #[test]
    fn seed_companion_fallible_with_extension_keeps_err_path() {
        let item: proc_macro2::TokenStream =
            "pub async fn get(Extension(ctx): Extension<Ctx>) -> Result<Json<X>, E> { todo!() }"
                .parse()
                .unwrap();
        let c = seed_companion(item, "/api/x").unwrap().to_string();
        assert!(c.contains("None => return None"), "{}", c);
        assert!(c.contains("Err (_) => None"), "{}", c);
    }
}
