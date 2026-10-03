//! The admin portal: request logs and background jobs, behind a root login.
//!
//! Mounted at `/__nx/admin` (feature `admin`). Like Coolify's root user, the
//! credentials come from the environment the app deploys with — there is no
//! user table:
//!
//! ```dotenv
//! NEXTRS_ADMIN_USER=drew
//! NEXTRS_ADMIN_PASSWORD_HASH=$argon2id$v=19$m=19456,t=2,p=1$...   # `nextrs admin set-password`
//! ```
//!
//! - **Fail-closed:** with either variable unset, every admin route answers
//!   404, so a forgotten env never exposes the dashboards.
//! - **A hash, not the password,** lives in the platform env. A plaintext
//!   `NEXTRS_ADMIN_PASSWORD` is accepted for local dev only (never on Vercel).
//! - **Sessions** are a signed cookie (12h). The signing key derives from the
//!   password hash, so changing the password logs every session out.
//!   `SameSite=Strict` covers CSRF for the Retry form.
//! - **Brute force:** argon2 makes each guess slow, and an IP is locked out for
//!   15 minutes after 10 failures. The counter is per instance (best effort on
//!   serverless, where instances don't share memory).
//!
//! Pages: `/__nx/admin/logs`, `/__nx/admin/logs/{id}`, `/__nx/admin/jobs`,
//! `/__nx/admin/jobs/{id}` (with Retry). JSON for scripts and the CLI, which
//! also accepts HTTP Basic with the same credentials:
//! `/__nx/admin/api/{logs,logs/{id},jobs,jobs/{id}}`,
//! `POST /__nx/admin/api/jobs/{id}/retry`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Mutex;

use axum::Router;
use axum::extract::{Form, Path, Query, Request};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use http::{HeaderMap, HeaderValue, StatusCode, header};
use serde::Deserialize;

use crate::jobs::{JobId, JobQuery, JobRow, JobStatus};
use crate::logs::{LogQuery, RequestLog};

/// Where the portal is mounted.
pub const ADMIN_PREFIX: &str = "/__nx/admin";
const COOKIE: &str = "nx_admin";
const SESSION_SECS: i64 = 12 * 3600;
const MAX_FAILURES: u32 = 10;
const LOCKOUT_MS: i64 = 15 * 60 * 1000;

// --------------------------------------------------------------- credentials

enum Secret {
    /// An argon2 PHC string.
    Hash(String),
    /// Dev-only plaintext.
    Plain(String),
}

struct Creds {
    user: String,
    secret: Secret,
}

impl Creds {
    fn secret_material(&self) -> &str {
        match &self.secret {
            Secret::Hash(h) | Secret::Plain(h) => h,
        }
    }
}

fn on_vercel() -> bool {
    std::env::var_os("VERCEL").is_some()
}

/// The configured root credentials, or `None` (the portal is then 404).
fn creds() -> Option<Creds> {
    let var = |n: &str| std::env::var(n).ok().filter(|v| !v.is_empty());
    let user = var("NEXTRS_ADMIN_USER")?;
    if let Some(hash) = var("NEXTRS_ADMIN_PASSWORD_HASH") {
        return Some(Creds { user, secret: Secret::Hash(hash) });
    }
    if on_vercel() {
        return None;
    }
    let plain = var("NEXTRS_ADMIN_PASSWORD")?;
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| {
        tracing::warn!(
            target: "nextrs::telemetry",
            "admin: using plaintext NEXTRS_ADMIN_PASSWORD (dev only) — run `nextrs admin set-password` for a hash"
        )
    });
    Some(Creds { user, secret: Secret::Plain(plain) })
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn verify(creds: &Creds, user: &str, password: &str) -> bool {
    let user_ok = constant_time_eq(user.as_bytes(), creds.user.as_bytes());
    let pass_ok = match &creds.secret {
        Secret::Hash(hash) => {
            use argon2::password_hash::{PasswordHash, PasswordVerifier};
            match PasswordHash::new(hash) {
                Ok(parsed) => argon2::Argon2::default()
                    .verify_password(password.as_bytes(), &parsed)
                    .is_ok(),
                Err(e) => {
                    tracing::error!(target: "nextrs::telemetry", error = %e,
                        "admin: NEXTRS_ADMIN_PASSWORD_HASH is not a valid argon2 hash");
                    false
                }
            }
        }
        Secret::Plain(plain) => constant_time_eq(password.as_bytes(), plain.as_bytes()),
    };
    user_ok && pass_ok
}

/// Hash a password for `NEXTRS_ADMIN_PASSWORD_HASH` (what `nextrs admin
/// set-password` calls).
pub fn hash_password(password: &str) -> Result<String, String> {
    use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
    let salt = SaltString::generate(&mut OsRng);
    argon2::Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ sessions

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn session_mac(creds: &Creds, exp: i64) -> String {
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};
    let key = Sha256::new()
        .chain_update(b"nextrs-admin-session\0")
        .chain_update(creds.user.as_bytes())
        .chain_update(b"\0")
        .chain_update(creds.secret_material().as_bytes())
        .finalize();
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("hmac accepts any key length");
    mac.update(format!("{}|{exp}", creds.user).as_bytes());
    hex(&mac.finalize().into_bytes())
}

fn sign_session(creds: &Creds, now_ms: i64) -> String {
    let exp = now_ms / 1000 + SESSION_SECS;
    format!("{exp}.{}", session_mac(creds, exp))
}

fn session_valid(creds: &Creds, value: &str, now_ms: i64) -> bool {
    let Some((exp, mac)) = value.split_once('.') else { return false };
    let Ok(exp) = exp.parse::<i64>() else { return false };
    exp > now_ms / 1000 && constant_time_eq(mac.as_bytes(), session_mac(creds, exp).as_bytes())
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| {
            let (k, v) = pair.trim().split_once('=')?;
            (k == name).then_some(v)
        })
}

fn basic_auth_ok(creds: &Creds, headers: &HeaderMap) -> bool {
    let Some(encoded) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
    else {
        return false;
    };
    let Some(decoded) = base64_decode(encoded.trim()) else { return false };
    let Ok(decoded) = String::from_utf8(decoded) else { return false };
    let Some((user, password)) = decoded.split_once(':') else { return false };
    verify(creds, user, password)
}

/// Minimal standard-alphabet base64 decoder (HTTP Basic only).
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= val(c)? << (18 - 6 * i);
        }
        let n = chunk.len();
        if n < 2 {
            return None;
        }
        out.push((acc >> 16) as u8);
        if n > 2 {
            out.push((acc >> 8) as u8);
        }
        if n > 3 {
            out.push(acc as u8);
        }
    }
    Some(out)
}

// ------------------------------------------------------------------- lockout

static FAILURES: Mutex<Option<HashMap<String, (u32, i64)>>> = Mutex::new(None);

fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .or_else(|| headers.get("x-real-ip").and_then(|v| v.to_str().ok()))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "local".into())
}

fn locked_out(ip: &str, now: i64) -> bool {
    let guard = FAILURES.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .and_then(|m| m.get(ip))
        .is_some_and(|(n, until)| *n >= MAX_FAILURES && *until > now)
}

fn record_failure(ip: &str, now: i64) {
    let mut guard = FAILURES.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    let entry = map.entry(ip.to_string()).or_insert((0, 0));
    if entry.1 <= now {
        *entry = (0, now + LOCKOUT_MS);
    }
    entry.0 += 1;
    entry.1 = now + LOCKOUT_MS;
}

fn clear_failures(ip: &str) {
    let mut guard = FAILURES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(map) = guard.as_mut() {
        map.remove(ip);
    }
}

// -------------------------------------------------------------------- router

/// The admin routes. Mounted by the router when the `admin` feature is on.
pub(crate) fn router() -> Router {
    Router::new()
        .route(ADMIN_PREFIX, get(|| async { Redirect::to(&format!("{ADMIN_PREFIX}/logs")) }))
        .route(&format!("{ADMIN_PREFIX}/login"), get(login_page).post(login))
        .route(&format!("{ADMIN_PREFIX}/logout"), post(logout))
        .route(&format!("{ADMIN_PREFIX}/logs"), get(logs_page))
        .route(&format!("{ADMIN_PREFIX}/logs/{{id}}"), get(log_page))
        .route(&format!("{ADMIN_PREFIX}/jobs"), get(jobs_page))
        .route(&format!("{ADMIN_PREFIX}/jobs/{{id}}"), get(job_page))
        .route(&format!("{ADMIN_PREFIX}/jobs/{{id}}/retry"), post(retry_form))
        .route(&format!("{ADMIN_PREFIX}/api/logs"), get(api_logs))
        .route(&format!("{ADMIN_PREFIX}/api/logs/{{id}}"), get(api_log))
        .route(&format!("{ADMIN_PREFIX}/api/jobs"), get(api_jobs))
        .route(&format!("{ADMIN_PREFIX}/api/jobs/{{id}}"), get(api_job))
        .route(&format!("{ADMIN_PREFIX}/api/jobs/{{id}}/retry"), post(api_retry))
        .layer(axum::middleware::from_fn(guard))
}

/// Fail-closed gate + hardening headers for every admin response.
async fn guard(req: Request, next: axum::middleware::Next) -> Response {
    if creds().is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("same-origin"));
    h.insert("x-robots-tag", HeaderValue::from_static("noindex"));
    resp
}

enum Access {
    Granted,
    Denied(Response),
}

/// Pages: a valid session cookie, else redirect to the login page.
fn page_access(headers: &HeaderMap, uri: &http::Uri) -> Access {
    let Some(creds) = creds() else {
        return Access::Denied(StatusCode::NOT_FOUND.into_response());
    };
    if cookie_value(headers, COOKIE).is_some_and(|v| session_valid(&creds, v, crate::logs::now_ms())) {
        return Access::Granted;
    }
    let next = uri.path_and_query().map(|p| p.as_str()).unwrap_or(ADMIN_PREFIX);
    Access::Denied(
        Redirect::to(&format!("{ADMIN_PREFIX}/login?next={}", url_encode(next))).into_response(),
    )
}

/// JSON API: session cookie or HTTP Basic (for scripts and the CLI).
fn api_access(headers: &HeaderMap) -> Access {
    let Some(creds) = creds() else {
        return Access::Denied(StatusCode::NOT_FOUND.into_response());
    };
    let now = crate::logs::now_ms();
    if cookie_value(headers, COOKIE).is_some_and(|v| session_valid(&creds, v, now)) {
        return Access::Granted;
    }
    let ip = client_ip(headers);
    if locked_out(&ip, now) {
        return Access::Denied((StatusCode::TOO_MANY_REQUESTS, "locked out").into_response());
    }
    if headers.contains_key(header::AUTHORIZATION) {
        if basic_auth_ok(&creds, headers) {
            clear_failures(&ip);
            return Access::Granted;
        }
        record_failure(&ip, now);
    }
    let mut resp = (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    resp.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"nextrs admin\""),
    );
    Access::Denied(resp)
}

macro_rules! require {
    ($access:expr) => {
        if let Access::Denied(resp) = $access {
            return resp;
        }
    };
}

// --------------------------------------------------------------------- login

#[derive(Deserialize, Default)]
struct NextParam {
    next: Option<String>,
}

/// Only redirect within the portal.
fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n) if n.starts_with(ADMIN_PREFIX) && !n.starts_with("//") => n.to_string(),
        _ => format!("{ADMIN_PREFIX}/logs"),
    }
}

async fn login_page(Query(q): Query<NextParam>) -> Response {
    login_form(q.next.as_deref(), None, StatusCode::OK)
}

fn login_form(next: Option<&str>, error: Option<&str>, status: StatusCode) -> Response {
    let next = safe_next(next);
    let error = error
        .map(|e| format!(r#"<p class="error">{}</p>"#, esc(e)))
        .unwrap_or_default();
    let body = format!(
        r#"<form class="login" method="post" action="{ADMIN_PREFIX}/login">
  <h1>nextrs admin</h1>
  {error}
  <label>Username <input name="user" autocomplete="username" autofocus required></label>
  <label>Password <input name="password" type="password" autocomplete="current-password" required></label>
  <input type="hidden" name="next" value="{}">
  <button type="submit">Sign in</button>
</form>"#,
        esc(&next)
    );
    (status, shell("Sign in", None, &body)).into_response()
}

#[derive(Deserialize)]
struct LoginForm {
    user: String,
    password: String,
    next: Option<String>,
}

async fn login(headers: HeaderMap, Form(form): Form<LoginForm>) -> Response {
    let Some(configured) = creds() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let now = crate::logs::now_ms();
    let ip = client_ip(&headers);
    if locked_out(&ip, now) {
        return login_form(
            form.next.as_deref(),
            Some("Too many failed attempts. Try again in 15 minutes."),
            StatusCode::TOO_MANY_REQUESTS,
        );
    }
    // argon2 is deliberately slow; keep it off the async workers.
    let (user, password) = (form.user.clone(), form.password.clone());
    let ok = tokio::task::spawn_blocking(move || verify(&configured, &user, &password))
        .await
        .unwrap_or(false);
    if !ok {
        record_failure(&ip, now);
        tracing::warn!(target: "nextrs::telemetry", ip = %ip, "admin: failed login");
        return login_form(form.next.as_deref(), Some("Wrong username or password."), StatusCode::UNAUTHORIZED);
    }
    clear_failures(&ip);
    // `creds` moved into the verifier; re-read (env lookups are cheap).
    let Some(creds) = creds() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let secure = if is_local(&headers) { "" } else { "; Secure" };
    let cookie = format!(
        "{COOKIE}={}; Path={ADMIN_PREFIX}; HttpOnly; SameSite=Strict; Max-Age={SESSION_SECS}{secure}",
        sign_session(&creds, now)
    );
    let mut resp = Redirect::to(&safe_next(form.next.as_deref())).into_response();
    if let Ok(v) = HeaderValue::from_str(&cookie) {
        resp.headers_mut().insert(header::SET_COOKIE, v);
    }
    resp
}

/// `Secure` cookies are dropped by some clients over plain-http localhost.
fn is_local(headers: &HeaderMap) -> bool {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|h| h.starts_with("localhost") || h.starts_with("127.0.0.1") || h.starts_with("[::1]"))
}

async fn logout() -> Response {
    let mut resp = Redirect::to(&format!("{ADMIN_PREFIX}/login")).into_response();
    resp.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!("{COOKIE}=; Path={ADMIN_PREFIX}; HttpOnly; SameSite=Strict; Max-Age=0"))
            .expect("static cookie"),
    );
    resp
}

// ---------------------------------------------------------------------- logs

#[derive(Deserialize, Default)]
struct LogParams {
    route: Option<String>,
    /// `5xx`, `4xx`, or a number (minimum status).
    status: Option<String>,
    level: Option<String>,
    /// `15m`, `1h`, `24h`, `7d`, …
    since: Option<String>,
    before: Option<i64>,
    limit: Option<u32>,
}

impl LogParams {
    fn query(&self) -> LogQuery {
        let nonempty = |o: &Option<String>| o.as_ref().filter(|s| !s.is_empty()).cloned();
        LogQuery {
            route: nonempty(&self.route),
            status_min: nonempty(&self.status).and_then(|s| {
                s.strip_suffix("xx")
                    .and_then(|d| d.parse::<u16>().ok().map(|d| d * 100))
                    .or_else(|| s.parse().ok())
            }),
            level: nonempty(&self.level),
            since: nonempty(&self.since)
                .and_then(|s| parse_duration_ms(&s))
                .map(|d| crate::logs::now_ms() - d),
            before: self.before,
            limit: Some(self.limit.unwrap_or(100)),
        }
    }
}

fn parse_duration_ms(s: &str) -> Option<i64> {
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit())?);
    let n: i64 = num.parse().ok()?;
    Some(n * match unit {
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return None,
    })
}

async fn logs_page(headers: HeaderMap, uri: http::Uri, Query(p): Query<LogParams>) -> Response {
    require!(page_access(&headers, &uri));
    let records = match crate::logs::store().query(p.query()).await {
        Ok(r) => r,
        Err(e) => return error_page(&e.to_string()),
    };
    let sel = |cur: &Option<String>, v: &str| if cur.as_deref() == Some(v) { " selected" } else { "" };
    let mut body = format!(
        r#"<form class="filters" method="get">
  <input name="route" placeholder="route, e.g. /api/todos" value="{route}">
  <select name="status"><option value="">any status</option><option value="4xx"{s4}>4xx+</option><option value="5xx"{s5}>5xx</option></select>
  <select name="level"><option value="">any level</option><option value="warn"{lw}>warn+</option><option value="error"{le}>error</option></select>
  <select name="since"><option value="">all time</option><option value="1h"{t1}>last hour</option><option value="24h"{t24}>last 24h</option><option value="7d"{t7}>last 7 days</option></select>
  <button>Filter</button> <a href="{ADMIN_PREFIX}/logs">reset</a>
</form>
<table><thead><tr><th>time</th><th>method</th><th>route</th><th>status</th><th class="num">ms</th><th>level</th><th class="num">lines</th></tr></thead><tbody>"#,
        route = esc(p.route.as_deref().unwrap_or("")),
        s4 = sel(&p.status, "4xx"),
        s5 = sel(&p.status, "5xx"),
        lw = sel(&p.level, "warn"),
        le = sel(&p.level, "error"),
        t1 = sel(&p.since, "1h"),
        t24 = sel(&p.since, "24h"),
        t7 = sel(&p.since, "7d"),
    );
    for r in &records {
        let _ = write!(
            body,
            r#"<tr class="link" onclick="location.href='{ADMIN_PREFIX}/logs/{id}'"><td>{time}</td><td>{method}</td><td><a href="{ADMIN_PREFIX}/logs/{id}">{route}</a></td><td>{status}</td><td class="num">{ms}</td><td>{level}</td><td class="num">{lines}</td></tr>"#,
            id = esc(&r.id),
            time = time_html(r.ts),
            method = esc(&r.method),
            route = esc(&r.route),
            status = status_badge(r.status),
            ms = fmt_ms(r.ms),
            level = r.level.as_deref().map(level_badge).unwrap_or_default(),
            lines = r.lines.len(),
        );
    }
    if records.is_empty() {
        body.push_str(r#"<tr><td colspan="7" class="empty">No requests match.</td></tr>"#);
    }
    body.push_str("</tbody></table>");
    if let Some(last) = records.last().filter(|_| records.len() as u32 >= p.limit.unwrap_or(100)) {
        let mut next = format!("{ADMIN_PREFIX}/logs?before={}", last.ts);
        for (k, v) in [("route", &p.route), ("status", &p.status), ("level", &p.level), ("since", &p.since)] {
            if let Some(v) = v.as_ref().filter(|v| !v.is_empty()) {
                let _ = write!(next, "&{k}={}", url_encode(v));
            }
        }
        let _ = write!(body, r#"<p class="more"><a href="{}">Older →</a></p>"#, esc(&next));
    }
    shell("Logs", Some("logs"), &body).into_response()
}

async fn log_page(headers: HeaderMap, uri: http::Uri, Path(id): Path<String>) -> Response {
    require!(page_access(&headers, &uri));
    let record = match crate::logs::store().get(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return not_found_page("No such request (it may have aged out)."),
        Err(e) => return error_page(&e.to_string()),
    };
    shell(&format!("{} {}", record.method, record.route), Some("logs"), &render_log(&record)).into_response()
}

fn render_log(r: &RequestLog) -> String {
    let mut body = format!(
        r#"<p><a href="{ADMIN_PREFIX}/logs">← logs</a></p>
<h1>{method} {route} {status}</h1>
<p class="meta">{time} · {ms} ms{cold} · <code>{id}</code></p>"#,
        method = esc(&r.method),
        route = esc(&r.route),
        status = status_badge(r.status),
        time = time_html(r.ts),
        ms = fmt_ms(r.ms),
        cold = if r.cold { " · cold start" } else { "" },
        id = esc(&r.id),
    );
    body.push_str(&waterfall(&r.segments, r.ms));
    body.push_str(&lines_table(&r.lines, r.dropped_lines));
    let _ = write!(
        body,
        "<details><summary>raw record</summary><pre>{}</pre></details>",
        esc(&serde_json::to_string_pretty(r).unwrap_or_default())
    );
    body
}

fn waterfall(segments: &[(String, f64)], total: f64) -> String {
    if segments.is_empty() {
        return String::new();
    }
    let scale = segments.iter().map(|(_, ms)| *ms).fold(total, f64::max).max(0.001);
    let mut out = String::from(r#"<h2>Timing</h2><div class="waterfall">"#);
    for (name, ms) in segments {
        let _ = write!(
            out,
            r#"<div class="seg"><span class="name">{}</span><span class="bar" style="width:{:.1}%"></span><span class="num">{:.1} ms</span></div>"#,
            esc(name),
            (ms / scale * 100.0).max(0.5),
            ms
        );
    }
    out.push_str("</div>");
    out
}

fn lines_table(lines: &[crate::logs::LogLine], dropped: usize) -> String {
    if lines.is_empty() {
        return r#"<h2>Log lines</h2><p class="empty">Nothing logged.</p>"#.into();
    }
    let mut out = String::from(
        r#"<h2>Log lines</h2><table class="lines"><thead><tr><th class="num">+ms</th><th>level</th><th>message</th><th>fields</th></tr></thead><tbody>"#,
    );
    for l in lines {
        let fields = l
            .fields
            .iter()
            .map(|(k, v)| {
                let v = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                format!("<span class=\"kv\">{}=<b>{}</b></span>", esc(k), esc(&v))
            })
            .collect::<Vec<_>>()
            .join(" ");
        let _ = write!(
            out,
            r#"<tr><td class="num">{:.0}</td><td>{}</td><td>{}{}</td><td>{}</td></tr>"#,
            l.t_ms,
            level_badge(&l.level),
            esc(&l.msg),
            if l.after_response { r#" <span class="tag">after response</span>"# } else { "" },
            fields
        );
    }
    out.push_str("</tbody></table>");
    if dropped > 0 {
        let _ = write!(out, r#"<p class="meta">{dropped} more lines were not kept.</p>"#);
    }
    out
}

// ---------------------------------------------------------------------- jobs

#[derive(Deserialize, Default)]
struct JobParams {
    status: Option<String>,
    name: Option<String>,
    before: Option<i64>,
    limit: Option<u32>,
}

impl JobParams {
    fn query(&self) -> JobQuery {
        JobQuery {
            status: self.status.as_deref().and_then(JobStatus::parse),
            name: self.name.clone().filter(|n| !n.is_empty()),
            before: self.before,
            limit: Some(self.limit.unwrap_or(50)),
        }
    }
}

async fn jobs_page(headers: HeaderMap, uri: http::Uri, Query(p): Query<JobParams>) -> Response {
    require!(page_access(&headers, &uri));
    let store = match crate::jobs::store() {
        Ok(s) => s,
        Err(e) => return error_page(&e.to_string()),
    };
    let rows = match store.list(p.query()).await {
        Ok(r) => r,
        Err(e) => return error_page(&e.to_string()),
    };
    let mut tabs = String::new();
    for (label, value) in [("all", ""), ("failed", "failed"), ("dead", "dead"), ("running", "running"), ("queued", "queued"), ("succeeded", "succeeded")] {
        let active = p.status.as_deref().unwrap_or("") == value;
        let _ = write!(
            tabs,
            r#"<a class="tab{}" href="{ADMIN_PREFIX}/jobs{}">{label}</a>"#,
            if active { " active" } else { "" },
            if value.is_empty() { String::new() } else { format!("?status={value}") }
        );
    }
    let mut body = format!(
        r#"<div class="tabs">{tabs}</div>
<table><thead><tr><th>created</th><th>job</th><th>id</th><th>status</th><th class="num">attempts</th><th>last error</th></tr></thead><tbody>"#
    );
    for r in &rows {
        let _ = write!(
            body,
            r#"<tr class="link" onclick="location.href='{ADMIN_PREFIX}/jobs/{id}'"><td>{time}</td><td>{name}</td><td><a href="{ADMIN_PREFIX}/jobs/{id}"><code>{short}</code></a></td><td>{status}</td><td class="num">{attempts}/{max}</td><td class="err">{err}</td></tr>"#,
            id = esc(&r.id.0),
            short = esc(&r.id.0[..r.id.0.len().min(8)]),
            time = time_html(r.created_at),
            name = esc(&r.name),
            status = job_badge(r.status),
            attempts = r.attempts,
            max = r.max_attempts,
            err = esc(&truncate(r.last_error.as_deref().unwrap_or(""), 80)),
        );
    }
    if rows.is_empty() {
        body.push_str(r#"<tr><td colspan="6" class="empty">No jobs.</td></tr>"#);
    }
    body.push_str("</tbody></table>");
    if let Some(last) = rows.last().filter(|_| rows.len() as u32 >= p.limit.unwrap_or(50)) {
        let status = p.status.as_deref().map(|s| format!("&status={}", url_encode(s))).unwrap_or_default();
        let _ = write!(body, r#"<p class="more"><a href="{ADMIN_PREFIX}/jobs?before={}{status}">Older →</a></p>"#, last.created_at);
    }
    shell("Jobs", Some("jobs"), &body).into_response()
}

async fn job_page(headers: HeaderMap, uri: http::Uri, Path(id): Path<String>) -> Response {
    require!(page_access(&headers, &uri));
    let store = match crate::jobs::store() {
        Ok(s) => s,
        Err(e) => return error_page(&e.to_string()),
    };
    let row = match store.get(&JobId(id)).await {
        Ok(Some(r)) => r,
        Ok(None) => return not_found_page("No such job (finished jobs age out)."),
        Err(e) => return error_page(&e.to_string()),
    };
    shell(&format!("{} {}", row.name, &row.id.0[..row.id.0.len().min(8)]), Some("jobs"), &render_job(&row)).into_response()
}

fn render_job(r: &JobRow) -> String {
    let retryable = !matches!(r.status, JobStatus::Queued | JobStatus::Running);
    let retry = if retryable {
        format!(
            r#"<form method="post" action="{ADMIN_PREFIX}/jobs/{}/retry"><button>Retry with the same payload</button></form>"#,
            esc(&r.id.0)
        )
    } else {
        String::new()
    };
    let next = r
        .next_run_at
        .map(|t| format!(" · next run {}", time_html(t)))
        .unwrap_or_default();
    let mut body = format!(
        r#"<p><a href="{ADMIN_PREFIX}/jobs">← jobs</a></p>
<h1>{name} {status}</h1>
<p class="meta">created {created}{next} · attempts {attempts}/{max} · <code>{id}</code></p>
{retry}
<div class="cols">
<div><h2>Payload</h2><pre>{payload}</pre></div>
<div><h2>Result</h2><pre>{result}</pre></div>
</div>"#,
        name = esc(&r.name),
        status = job_badge(r.status),
        created = time_html(r.created_at),
        attempts = r.attempts,
        max = r.max_attempts,
        id = esc(&r.id.0),
        payload = esc(&serde_json::to_string_pretty(&r.payload).unwrap_or_default()),
        result = esc(&r
            .result
            .as_ref()
            .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
            .unwrap_or_else(|| "—".into())),
    );
    body.push_str("<h2>Attempts</h2>");
    if r.history.is_empty() {
        body.push_str(r#"<p class="empty">No finished attempts yet.</p>"#);
    }
    for a in r.history.iter().rev() {
        let outcome = match &a.error {
            None => r#"<span class="badge ok">ok</span>"#.to_string(),
            Some(e) => format!(r#"<span class="badge err">failed</span> <span class="err">{}</span>"#, esc(e)),
        };
        let _ = write!(
            body,
            r#"<details class="attempt"{open}><summary>attempt {n} · {outcome} · {ms} ms · {time}</summary>{lines}</details>"#,
            open = if a.n == r.attempts { " open" } else { "" },
            n = a.n,
            ms = a.ms,
            time = time_html(a.started_at),
            lines = lines_table(&a.lines, a.dropped_lines),
        );
    }
    body
}

async fn retry_form(headers: HeaderMap, uri: http::Uri, Path(id): Path<String>) -> Response {
    require!(page_access(&headers, &uri));
    match crate::jobs::retry(&JobId(id.clone())).await {
        Ok(_) => Redirect::to(&format!("{ADMIN_PREFIX}/jobs/{}", url_encode(&id))).into_response(),
        Err(e) => error_page(&e.to_string()),
    }
}

// ----------------------------------------------------------------------- api

fn json_err(status: StatusCode, msg: &str) -> Response {
    (status, axum::Json(serde_json::json!({ "error": msg }))).into_response()
}

async fn api_logs(headers: HeaderMap, Query(p): Query<LogParams>) -> Response {
    require!(api_access(&headers));
    match crate::logs::store().query(p.query()).await {
        Ok(records) => axum::Json(serde_json::json!({ "records": records })).into_response(),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn api_log(headers: HeaderMap, Path(id): Path<String>) -> Response {
    require!(api_access(&headers));
    match crate::logs::store().get(&id).await {
        Ok(Some(r)) => axum::Json(r).into_response(),
        Ok(None) => json_err(StatusCode::NOT_FOUND, "no such request"),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn api_jobs(headers: HeaderMap, Query(p): Query<JobParams>) -> Response {
    require!(api_access(&headers));
    let result = async { crate::jobs::store()?.list(p.query()).await }.await;
    match result {
        Ok(jobs) => axum::Json(serde_json::json!({ "jobs": jobs })).into_response(),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn api_job(headers: HeaderMap, Path(id): Path<String>) -> Response {
    require!(api_access(&headers));
    let result = async { crate::jobs::store()?.get(&JobId(id)).await }.await;
    match result {
        Ok(Some(row)) => axum::Json(row).into_response(),
        Ok(None) => json_err(StatusCode::NOT_FOUND, "no such job"),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn api_retry(headers: HeaderMap, Path(id): Path<String>) -> Response {
    require!(api_access(&headers));
    match crate::jobs::retry(&JobId(id)).await {
        Ok(Some(row)) => (StatusCode::ACCEPTED, axum::Json(row)).into_response(),
        Ok(None) => json_err(StatusCode::CONFLICT, "job is missing, queued, or running"),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

// ---------------------------------------------------------------------- html

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn url_encode(s: &str) -> String {
    s.bytes().fold(String::new(), |mut out, b| {
        if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
        out
    })
}

/// `0.3`, `12`, `1840` — a decimal only where it carries information.
fn fmt_ms(ms: f64) -> String {
    if ms < 10.0 { format!("{ms:.1}") } else { format!("{ms:.0}") }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// `2026-10-03 04:48:30 UTC` (a script rewrites it into the viewer's zone).
fn fmt_utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        sod / 3600,
        sod % 3600 / 60,
        sod % 60
    )
}

fn time_html(ms: i64) -> String {
    format!(r#"<time data-ms="{ms}">{}</time>"#, fmt_utc(ms))
}

fn status_badge(status: u16) -> String {
    let class = match status {
        500.. => "err",
        400..=499 => "warn",
        _ => "ok",
    };
    format!(r#"<span class="badge {class}">{status}</span>"#)
}

fn level_badge(level: &str) -> String {
    let class = match level {
        "error" => "err",
        "warn" => "warn",
        _ => "muted",
    };
    format!(r#"<span class="badge {class}">{}</span>"#, esc(level))
}

fn job_badge(status: JobStatus) -> String {
    let class = match status {
        JobStatus::Succeeded => "ok",
        JobStatus::Failed => "warn",
        JobStatus::Dead => "err",
        JobStatus::Running => "run",
        JobStatus::Queued => "muted",
    };
    format!(r#"<span class="badge {class}">{}</span>"#, status.as_str())
}

fn not_found_page(msg: &str) -> Response {
    (StatusCode::NOT_FOUND, shell("Not found", None, &format!(r#"<p class="empty">{}</p>"#, esc(msg)))).into_response()
}

fn error_page(msg: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        shell("Error", None, &format!(r#"<p class="error">{}</p>"#, esc(msg))),
    )
        .into_response()
}

fn shell(title: &str, active: Option<&str>, body: &str) -> Html<String> {
    let nav = match active {
        Some(active) => {
            let link = |key: &str, label: &str| {
                format!(
                    r#"<a href="{ADMIN_PREFIX}/{key}"{}>{label}</a>"#,
                    if key == active { r#" class="active""# } else { "" }
                )
            };
            format!(
                r#"<nav><span class="brand">nextrs admin</span>{}{}<form method="post" action="{ADMIN_PREFIX}/logout"><button class="link">sign out</button></form></nav>"#,
                link("logs", "Logs"),
                link("jobs", "Jobs")
            )
        }
        None => String::new(),
    };
    Html(format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex"><title>{title} · nextrs admin</title>
<style>{CSS}</style></head>
<body>{nav}<main>{body}</main>
<script>for(const t of document.querySelectorAll('time[data-ms]')){{const d=new Date(+t.dataset.ms);t.title=t.textContent;t.textContent=d.toLocaleString();}}</script>
</body></html>"#,
        title = esc(title)
    ))
}

const CSS: &str = r#"
:root{--bg:#fff;--fg:#1a1a1a;--muted:#6b6b6b;--line:#e6e6e6;--row:#f7f7f7;--accent:#2563eb;--ok:#15803d;--warn:#b45309;--err:#b91c1c;--run:#7c3aed}
@media (prefers-color-scheme:dark){:root{--bg:#111;--fg:#e8e8e8;--muted:#9a9a9a;--line:#2a2a2a;--row:#1a1a1a;--accent:#60a5fa;--ok:#4ade80;--warn:#fbbf24;--err:#f87171;--run:#a78bfa}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.45 ui-sans-serif,system-ui,sans-serif}
nav{display:flex;gap:16px;align-items:center;padding:10px 16px;border-bottom:1px solid var(--line)}
nav .brand{font-weight:600;margin-right:8px}nav a{color:var(--muted);text-decoration:none}nav a.active{color:var(--fg);font-weight:600}
nav form{margin-left:auto}button.link{background:none;border:0;color:var(--muted);cursor:pointer;font:inherit;padding:0}
main{padding:16px;max-width:1200px;margin:0 auto}h1{font-size:20px;margin:8px 0}h2{font-size:15px;margin:20px 0 8px}
a{color:var(--accent)}code,pre,td.num,.waterfall .num{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:12.5px}
table{width:100%;border-collapse:collapse}th{text-align:left;color:var(--muted);font-weight:500;border-bottom:1px solid var(--line);padding:6px 8px}
td{padding:6px 8px;border-bottom:1px solid var(--line);vertical-align:top}tr.link{cursor:pointer}tr.link:hover{background:var(--row)}
td.num,th.num{text-align:right}.empty{color:var(--muted);text-align:center;padding:24px}
.badge{display:inline-block;padding:0 7px;border-radius:9px;font-size:12px;border:1px solid currentColor}
.ok{color:var(--ok)}.warn{color:var(--warn)}.err{color:var(--err)}.run{color:var(--run)}.muted{color:var(--muted)}
.tag{font-size:11px;color:var(--muted);border:1px dashed var(--line);padding:0 5px;border-radius:6px}
.kv{margin-right:10px;font-family:ui-monospace,monospace;font-size:12px;color:var(--muted)}.kv b{color:var(--fg);font-weight:500}
.filters{display:flex;flex-wrap:wrap;gap:8px;align-items:center;margin-bottom:12px}
input,select,button{font:inherit;padding:5px 8px;border:1px solid var(--line);border-radius:6px;background:var(--bg);color:var(--fg)}
button{cursor:pointer}form.login{max-width:320px;margin:12vh auto;display:flex;flex-direction:column;gap:12px}
form.login label{display:flex;flex-direction:column;gap:4px;color:var(--muted)}p.error{color:var(--err)}
.meta{color:var(--muted)}pre{background:var(--row);padding:10px;border-radius:6px;overflow:auto;max-height:420px}
.waterfall .seg{display:grid;grid-template-columns:120px 1fr 90px;gap:8px;align-items:center;margin:3px 0}
.waterfall .bar{height:10px;background:var(--accent);border-radius:3px;display:block}.waterfall .num{text-align:right;color:var(--muted)}
.tabs{display:flex;gap:4px;margin-bottom:12px}.tab{padding:4px 10px;border-radius:6px;text-decoration:none;color:var(--muted)}.tab.active{background:var(--row);color:var(--fg)}
.cols{display:grid;grid-template-columns:1fr 1fr;gap:16px}@media (max-width:700px){.cols{grid-template-columns:1fr}}
details.attempt{border:1px solid var(--line);border-radius:6px;padding:6px 10px;margin:6px 0}details summary{cursor:pointer}
.more{text-align:center}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(user: &str, pass: &str) -> Creds {
        Creds { user: user.into(), secret: Secret::Plain(pass.into()) }
    }

    #[test]
    fn sessions_sign_verify_expire_and_rotate() {
        let c = plain("drew", "pw");
        let now = 1_800_000_000_000;
        let s = sign_session(&c, now);
        assert!(session_valid(&c, &s, now));
        assert!(!session_valid(&c, &s, now + (SESSION_SECS + 1) * 1000), "expired");
        assert!(!session_valid(&plain("drew", "new-pw"), &s, now), "password change invalidates");
        let (exp, mac) = s.split_once('.').unwrap();
        let forged = format!("{}.{mac}", exp.parse::<i64>().unwrap() + 3600);
        assert!(!session_valid(&c, &forged, now), "extended expiry must not verify");
    }

    #[test]
    fn argon2_hash_round_trip() {
        let hash = hash_password("hunter2").unwrap();
        let c = Creds { user: "drew".into(), secret: Secret::Hash(hash) };
        assert!(verify(&c, "drew", "hunter2"));
        assert!(!verify(&c, "drew", "hunter3"));
        assert!(!verify(&c, "root", "hunter2"));
    }

    #[test]
    fn basic_auth_and_cookie_parsing() {
        let c = plain("drew", "pw");
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic ZHJldzpwdw==")); // drew:pw
        assert!(basic_auth_ok(&c, &h));
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic ZHJldzp4")); // drew:x
        assert!(!basic_auth_ok(&c, &h));
        h.insert(header::COOKIE, HeaderValue::from_static("a=1; nx_admin=abc.def; b=2"));
        assert_eq!(cookie_value(&h, COOKIE), Some("abc.def"));
    }

    #[test]
    fn redirects_stay_inside_the_portal() {
        assert_eq!(safe_next(Some("/__nx/admin/jobs?status=dead")), "/__nx/admin/jobs?status=dead");
        assert_eq!(safe_next(Some("https://evil.example")), "/__nx/admin/logs");
        assert_eq!(safe_next(Some("//evil.example")), "/__nx/admin/logs");
        assert_eq!(safe_next(None), "/__nx/admin/logs");
    }

    #[test]
    fn lockout_after_repeated_failures() {
        let ip = "203.0.113.9";
        let now = 1_000;
        for _ in 0..MAX_FAILURES {
            assert!(!locked_out(ip, now));
            record_failure(ip, now);
        }
        assert!(locked_out(ip, now));
        assert!(!locked_out(ip, now + LOCKOUT_MS + 1), "lockout expires");
        clear_failures(ip);
    }

    #[test]
    fn helpers() {
        assert_eq!(fmt_utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(fmt_utc(1_791_002_910_201), "2026-10-03 04:48:30 UTC");
        assert_eq!(parse_duration_ms("24h"), Some(86_400_000));
        assert_eq!(esc("<a href=\"x\">"), "&lt;a href=&quot;x&quot;&gt;");
        let p = LogParams { status: Some("5xx".into()), ..Default::default() };
        assert_eq!(p.query().status_min, Some(500));
    }
}
