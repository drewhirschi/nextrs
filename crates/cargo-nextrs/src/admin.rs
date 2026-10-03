//! `nextrs admin set-password`, `nextrs logs`, and `nextrs jobs` — the
//! terminal side of the admin portal (`/__nx/admin`, framework feature
//! `admin`).
//!
//! `logs`/`jobs` call the deployed app's JSON API (`/__nx/admin/api/*`) with
//! HTTP Basic. The URL is `--url` or `app.url` from `nextrs.toml`; the
//! username/password come from `NEXTRS_ADMIN_USER` / `NEXTRS_ADMIN_PASSWORD`
//! (process env, then the same env files `deploy` reads), else a prompt.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct QueryOptions {
    pub root: Option<PathBuf>,
    pub url: Option<String>,
    /// `key=value` query parameters for the API.
    pub params: Vec<(String, String)>,
    pub id: Option<String>,
    pub json: bool,
}

// ------------------------------------------------------------- set-password

/// Prompt for a username and password, hash it (argon2id), and upsert
/// `NEXTRS_ADMIN_USER` + `NEXTRS_ADMIN_PASSWORD_HASH` into `<root>/.env.local`.
pub fn set_password(root: &Path, user: Option<String>) -> Result<(), String> {
    let user = match user {
        Some(u) => u,
        None => {
            let default = std::env::var("USER").unwrap_or_else(|_| "admin".into());
            let typed = prompt(&format!("Username [{default}]: "), false)?;
            if typed.is_empty() { default } else { typed }
        }
    };
    let password = prompt("Password: ", true)?;
    if password.len() < 8 {
        return Err("password must be at least 8 characters".into());
    }
    if std::io::stdin().is_terminal() && prompt("Repeat password: ", true)? != password {
        return Err("passwords did not match".into());
    }
    let hash = hash_password(&password)?;

    let path = root.join(".env.local");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = upsert_env(
        &existing,
        &[
            ("NEXTRS_ADMIN_USER", user.clone()),
            // Single quotes: dotenv expands `$` inside double quotes.
            ("NEXTRS_ADMIN_PASSWORD_HASH", format!("'{hash}'")),
        ],
    );
    std::fs::write(&path, updated).map_err(|e| format!("{}: {e}", path.display()))?;
    eprintln!("nextrs: wrote NEXTRS_ADMIN_USER and NEXTRS_ADMIN_PASSWORD_HASH to {}", path.display());
    eprintln!(
        "\nAdd them to the deployment's env (the hash, never the password):\n\n  \
         vercel env add NEXTRS_ADMIN_USER production        # {user}\n  \
         vercel env add NEXTRS_ADMIN_PASSWORD_HASH production\n\n\
         The portal is at <app.url>/__nx/admin once the app is built with nextrs's `admin` feature.\n\
         `nextrs logs` / `nextrs jobs` sign in with NEXTRS_ADMIN_USER plus the password\n\
         (NEXTRS_ADMIN_PASSWORD in your shell or a local env file, or a prompt)."
    );
    Ok(())
}

fn hash_password(password: &str) -> Result<String, String> {
    use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
    let salt = SaltString::generate(&mut OsRng);
    argon2::Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

/// Replace `KEY=...` lines (or append them), keeping everything else.
fn upsert_env(existing: &str, pairs: &[(&str, String)]) -> String {
    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();
    for (key, value) in pairs {
        let line = format!("{key}={value}");
        match lines
            .iter()
            .position(|l| l.trim_start().strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('=')))
        {
            Some(i) => lines[i] = line,
            None => lines.push(line),
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Read one line from stdin; `secret` turns terminal echo off while typing.
fn prompt(label: &str, secret: bool) -> Result<String, String> {
    let tty = std::io::stdin().is_terminal();
    if tty {
        eprint!("{label}");
        let _ = std::io::stderr().flush();
    }
    let echo_off = secret && tty && stty("-echo");
    let mut line = String::new();
    let read = std::io::stdin().read_line(&mut line);
    if echo_off {
        stty("echo");
        eprintln!();
    }
    read.map_err(|e| e.to_string())?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn stty(arg: &str) -> bool {
    std::process::Command::new("stty")
        .arg(arg)
        .stdin(std::process::Stdio::inherit())
        .status()
        .is_ok_and(|s| s.success())
}

// ---------------------------------------------------------------- api client

struct Client {
    base: String,
    auth: String,
}

impl Client {
    fn from_options(opts: &QueryOptions) -> Result<Self, String> {
        let root = crate::cron::resolve_root(opts.root.clone())?;
        let base = match &opts.url {
            Some(url) => url.clone(),
            None => crate::cron::load_config(&root)
                .map_err(|e| format!("{e}\n  (pass --url <app url> to skip nextrs.toml)"))?
                .app
                .url,
        };
        let config = crate::cron::load_config(&root).ok();
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        crate::env_file::load(
            &root,
            Some(&cwd),
            crate::env_file::Target::Production,
            config.as_ref().and_then(|c| c.deploy.as_ref()),
        )?;
        let user = std::env::var("NEXTRS_ADMIN_USER")
            .ok()
            .filter(|v| !v.is_empty())
            .ok_or("NEXTRS_ADMIN_USER is not set (run `nextrs admin set-password`, or export it)")?;
        let password = match std::env::var("NEXTRS_ADMIN_PASSWORD").ok().filter(|v| !v.is_empty()) {
            Some(p) => p,
            None if std::io::stdin().is_terminal() => prompt(&format!("Password for {user}: "), true)?,
            None => return Err("NEXTRS_ADMIN_PASSWORD is not set and stdin is not a terminal".into()),
        };
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            auth: format!("Basic {}", base64_encode(format!("{user}:{password}").as_bytes())),
        })
    }

    fn call(&self, method: &str, path: &str, params: &[(String, String)]) -> Result<Value, String> {
        let url = format!("{}/__nx/admin/api{path}", self.base);
        let mut req = ureq::request(method, &url).set("authorization", &self.auth);
        for (k, v) in params {
            req = req.query(k, v);
        }
        match req.call() {
            Ok(resp) => {
                let text = resp.into_string().map_err(|e| e.to_string())?;
                serde_json::from_str(&text).map_err(|e| format!("{url}: not JSON: {e}"))
            }
            Err(ureq::Error::Status(401, _)) => Err("401 — wrong NEXTRS_ADMIN_USER / password".into()),
            Err(ureq::Error::Status(404, _)) if path.matches('/').count() <= 1 => Err(format!(
                "{url} answered 404 — the admin portal is off: build the app with nextrs's `admin` \
                 feature and set NEXTRS_ADMIN_USER + NEXTRS_ADMIN_PASSWORD_HASH in its env"
            )),
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                Err(format!("{url} answered {code}: {body}"))
            }
            Err(e) => Err(format!("{url}: {e}")),
        }
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ---------------------------------------------------------------------- logs

pub fn logs(opts: &QueryOptions) -> Result<(), String> {
    let client = Client::from_options(opts)?;
    if let Some(id) = &opts.id {
        let record = client.call("GET", &format!("/logs/{id}"), &[])?;
        return print(&record, opts.json, print_log_detail);
    }
    let body = client.call("GET", "/logs", &opts.params)?;
    print(&body, opts.json, |body| {
        let records = body["records"].as_array().cloned().unwrap_or_default();
        if records.is_empty() {
            println!("no requests match");
        }
        for r in records.iter().rev() {
            let last = r["lines"]
                .as_array()
                .and_then(|lines| {
                    lines
                        .iter()
                        .filter(|l| matches!(l["level"].as_str(), Some("error" | "warn")))
                        .last()
                        .or(lines.last())
                })
                .map(|l| line_text(l))
                .unwrap_or_default();
            println!(
                "{}  {:<6} {:<28} {:>3} {:>7}  {:<5}  {}  {}",
                utc(r["ts"].as_i64().unwrap_or(0)),
                r["method"].as_str().unwrap_or(""),
                r["route"].as_str().unwrap_or(""),
                r["status"],
                format!("{:.1}ms", r["ms"].as_f64().unwrap_or(0.0)),
                r["level"].as_str().unwrap_or("-"),
                r["id"].as_str().unwrap_or(""),
                last
            );
        }
    })
}

fn print_log_detail(r: &Value) {
    println!(
        "{} {} → {}  {:.1}ms  {}{}",
        r["method"].as_str().unwrap_or(""),
        r["route"].as_str().unwrap_or(""),
        r["status"],
        r["ms"].as_f64().unwrap_or(0.0),
        utc(r["ts"].as_i64().unwrap_or(0)),
        if r["cold"].as_bool() == Some(true) { "  (cold start)" } else { "" }
    );
    if let Some(segments) = r["segments"].as_array() {
        let parts: Vec<String> = segments
            .iter()
            .filter_map(|s| Some(format!("{}={:.1}ms", s[0].as_str()?, s[1].as_f64()?)))
            .collect();
        println!("timing: {}", parts.join("  "));
    }
    for l in r["lines"].as_array().into_iter().flatten() {
        println!(
            "  +{:>6.1}ms  {:<5} {}{}",
            l["t_ms"].as_f64().unwrap_or(0.0),
            l["level"].as_str().unwrap_or(""),
            line_text(l),
            if l["after_response"].as_bool() == Some(true) { "  (after response)" } else { "" }
        );
    }
}

fn line_text(l: &Value) -> String {
    let mut text = l["msg"].as_str().unwrap_or("").to_string();
    if let Some(fields) = l["fields"].as_object() {
        for (k, v) in fields {
            let v = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
            text.push_str(&format!(" {k}={v}"));
        }
    }
    text
}

// ---------------------------------------------------------------------- jobs

pub fn jobs(opts: &QueryOptions) -> Result<(), String> {
    let client = Client::from_options(opts)?;
    if let Some(id) = &opts.id {
        let row = client.call("GET", &format!("/jobs/{id}"), &[])?;
        return print(&row, opts.json, print_job_detail);
    }
    let body = client.call("GET", "/jobs", &opts.params)?;
    print(&body, opts.json, |body| {
        let jobs = body["jobs"].as_array().cloned().unwrap_or_default();
        if jobs.is_empty() {
            println!("no jobs");
        }
        for j in jobs.iter().rev() {
            println!(
                "{}  {:<20} {:<9} {}/{}  {}  {}",
                utc(j["created_at"].as_i64().unwrap_or(0)),
                j["name"].as_str().unwrap_or(""),
                j["status"].as_str().unwrap_or(""),
                j["attempts"],
                j["max_attempts"],
                j["id"].as_str().unwrap_or(""),
                // An earlier attempt's error is noise once the job succeeded.
                match j["status"].as_str() {
                    Some("failed" | "dead") => j["last_error"].as_str().unwrap_or(""),
                    _ => "",
                }
            );
        }
    })
}

pub fn retry_job(opts: &QueryOptions) -> Result<(), String> {
    let id = opts.id.as_deref().ok_or("usage: nextrs jobs retry <ID>")?;
    let client = Client::from_options(opts)?;
    let row = client.call("POST", &format!("/jobs/{id}/retry"), &[])?;
    eprintln!(
        "nextrs: requeued {} ({}); it runs now — `nextrs jobs {id}` to follow it",
        row["name"].as_str().unwrap_or(""),
        id
    );
    Ok(())
}

fn print_job_detail(j: &Value) {
    println!(
        "{} {}  {}  attempts {}/{}  created {}",
        j["name"].as_str().unwrap_or(""),
        j["id"].as_str().unwrap_or(""),
        j["status"].as_str().unwrap_or(""),
        j["attempts"],
        j["max_attempts"],
        utc(j["created_at"].as_i64().unwrap_or(0))
    );
    println!("payload: {}", j["payload"]);
    if !j["result"].is_null() {
        println!("result:  {}", j["result"]);
    }
    for a in j["history"].as_array().into_iter().flatten() {
        let outcome = a["error"].as_str().map(|e| format!("failed: {e}")).unwrap_or_else(|| "ok".into());
        println!("attempt {} · {} · {}ms · {}", a["n"], outcome, a["ms"], utc(a["started_at"].as_i64().unwrap_or(0)));
        for l in a["lines"].as_array().into_iter().flatten() {
            println!("  +{:>6.1}ms  {:<5} {}", l["t_ms"].as_f64().unwrap_or(0.0), l["level"].as_str().unwrap_or(""), line_text(l));
        }
    }
}

fn print(value: &Value, json: bool, human: impl FnOnce(&Value)) -> Result<(), String> {
    if json {
        println!("{}", serde_json::to_string_pretty(value).map_err(|e| e.to_string())?);
    } else {
        human(value);
    }
    Ok(())
}

/// `2026-10-03 04:48:30Z`.
fn utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", sod / 3600, sod % 3600 / 60, sod % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_replaces_and_appends() {
        let out = upsert_env(
            "# keep\nNEXTRS_ADMIN_USER=old\nOTHER=1\n",
            &[("NEXTRS_ADMIN_USER", "drew".into()), ("NEXTRS_ADMIN_PASSWORD_HASH", "'h'".into())],
        );
        assert_eq!(out, "# keep\nNEXTRS_ADMIN_USER=drew\nOTHER=1\nNEXTRS_ADMIN_PASSWORD_HASH='h'\n");
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64_encode(b"drew:pw"), "ZHJldzpwdw==");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"abc"), "YWJj");
    }

    #[test]
    fn hash_is_argon2id_phc() {
        assert!(hash_password("correct horse").unwrap().starts_with("$argon2id$"));
    }

    #[test]
    fn utc_formats() {
        assert_eq!(utc(1_791_002_910_201), "2026-10-03 04:48:30Z");
    }
}
