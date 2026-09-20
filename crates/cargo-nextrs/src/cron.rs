//! Cron declaration, codegen, and deploy.
//!
//! Apps declare schedules on `#[nextrs::cron]` route handlers; the CLI scans
//! `app/**/route.rs` and turns those declarations into provider plumbing:
//!
//! - `cloudflare` crons become a generated Worker (`.nextrs/cloudflare/`)
//!   whose `scheduled()` handler does nothing but fetch the app's route on
//!   Vercel with `Authorization: Bearer $CRON_SECRET`. All real logic stays
//!   in the Rust app; the Worker is disposable plumbing.
//! - `vercel` crons are written into the framework-owned
//!   `.nextrs/vercel.json` used by `nextrs deploy`.
//!
//! Vercel is the default provider. Subdaily schedules produce a Hobby-plan
//! warning with an explicit `provider = "cloudflare"` alternative.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::{Value, json};

pub const CONFIG_FILE: &str = "nextrs.toml";
pub const OUTPUT_DIR: &str = ".nextrs/cloudflare";
pub const VERCEL_CONFIG_FILE: &str = ".nextrs/vercel.json";
const GENERATED_README: &str = ".nextrs/README.md";

/// Wrangler pins Worker runtime behavior to this date; bump deliberately.
const COMPATIBILITY_DATE: &str = "2026-08-01";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextrsConfig {
    pub app: AppConfig,
    #[serde(default)]
    #[allow(dead_code)]
    pub bundles: std::collections::BTreeMap<String, nextrs::server_bundles::BundleSettings>,
    #[serde(default)]
    #[allow(dead_code)]
    pub deployment: nextrs::server_bundles::DeploymentSettings,
    /// Optional Vercel overrides. Framework defaults are used when absent.
    pub vercel: Option<VercelConfig>,
    /// Optional deploy-command settings (`env_file`).
    pub deploy: Option<crate::env_file::DeployConfig>,
    /// Optional custom build step for `nextrs deploy`.
    pub build: Option<BuildConfig>,
}

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct BuildConfig {
    /// Shell command `nextrs deploy` runs from the app root instead of
    /// compiling the server bundles itself. It must leave a complete bundle
    /// output in `$NEXTRS_BUNDLE_OUTPUT`; nextrs verifies it before upload.
    pub command: Option<String>,
}

/// The knobs a nextrs app's `vercel.json` actually varies on. Everything
/// else (the Rust function, the catch-all rewrite, immutable `/dist` caching)
/// is the framework's fixed deploy shape.
#[derive(Debug, Default, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct VercelConfig {
    /// Function regions, e.g. `["pdx1"]`. Omit for Vercel's default.
    #[serde(default)]
    pub regions: Vec<String>,
    /// vercel-rust runtime pin. Default: [`DEFAULT_VERCEL_RUNTIME`].
    pub runtime: Option<String>,
    /// Default: `npm ci`.
    pub install_command: Option<String>,
    /// Default: [`DEFAULT_BUILD_COMMAND`].
    pub build_command: Option<String>,
    /// Git-push auto-builds. Default `false` — nextrs apps deploy prebuilt.
    pub git_deploys: Option<bool>,
    /// Raw top-level keys for Vercel features NextRS does not model. Keys
    /// owned by the framework are rejected rather than overridden.
    #[serde(default)]
    pub extra: toml::Table,
}

pub const DEFAULT_VERCEL_RUNTIME: &str = "vercel-rust@4.0.11";
pub const DEFAULT_BUILD_COMMAND: &str =
    "npm run client:prepare && cargo build --release --bin index && npm run client:build";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// Used to name the generated Worker (`<name>-cron`).
    pub name: String,
    /// The deployed app's base URL, e.g. `https://myapp.vercel.app`.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronEntry {
    /// Route path on the app, e.g. `/api/cron/refresh`.
    pub path: String,
    /// Five-field cron expression.
    pub schedule: String,
    /// `cloudflare` | `vercel`; Vercel is the default.
    pub provider: Provider,
    pub source: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Cloudflare,
    Vercel,
}

impl CronEntry {
    pub fn provider(&self) -> Provider {
        self.provider
    }
}

/// A schedule is daily-or-coarser when its minute and hour fields are plain
/// numbers — anything with `*`, steps, ranges, or lists in those fields fires
/// more than once a day and exceeds what Vercel Hobby's native cron offers.
fn is_daily_or_coarser(schedule: &str) -> bool {
    let mut fields = schedule.split_whitespace();
    let (Some(minute), Some(hour)) = (fields.next(), fields.next()) else {
        return false;
    };
    minute.parse::<u8>().is_ok() && hour.parse::<u8>().is_ok()
}

pub fn load_config(root: &Path) -> Result<NextrsConfig, String> {
    let path = root.join(CONFIG_FILE);
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let config: NextrsConfig =
        toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    if !config.app.url.starts_with("https://") && !config.app.url.starts_with("http://") {
        return Err(format!(
            "{}: app.url `{}` must be an absolute http(s) URL",
            path.display(),
            config.app.url
        ));
    }
    Ok(config)
}

/// Discover deployment declarations colocated with protected cron handlers.
pub fn discover_crons(root: &Path) -> Result<Vec<CronEntry>, String> {
    let app = root.join("app");
    let mut route_files = Vec::new();
    find_named_files(&app, "route.rs", &mut route_files);
    let mut crons = Vec::new();
    for file in route_files {
        let source = fs::read_to_string(&file)
            .map_err(|error| format!("failed to read {}: {error}", file.display()))?;
        let syntax = syn::parse_file(&source)
            .map_err(|error| format!("{}: cannot parse route source: {error}", file.display()))?;
        for item in syntax.items {
            let syn::Item::Fn(function) = item else {
                continue;
            };
            for attr in function.attrs.iter().filter(|attr| {
                let segments = &attr.path().segments;
                segments
                    .last()
                    .is_some_and(|segment| segment.ident == "cron")
            }) {
                if function.sig.ident != "get" {
                    return Err(format!(
                        "{}: #[nextrs::cron] handler must be named `get`",
                        file.display()
                    ));
                }
                let parser = syn::punctuated::Punctuated::<syn::MetaNameValue, syn::Token![,]>::parse_terminated;
                let args = attr.parse_args_with(parser).map_err(|error| {
                    format!("{}: invalid #[nextrs::cron(...)]: {error}", file.display())
                })?;
                let mut schedule = None;
                let mut provider = Provider::Vercel;
                let mut disabled = None;
                for arg in args {
                    let name = arg
                        .path
                        .get_ident()
                        .map(ToString::to_string)
                        .ok_or_else(|| {
                            format!(
                                "{}: expected cron `schedule`, `provider`, or `disabled`",
                                file.display()
                            )
                        })?;
                    match name.as_str() {
                        "schedule" | "provider" => {
                            let syn::Expr::Lit(syn::ExprLit {
                                lit: syn::Lit::Str(value),
                                ..
                            }) = arg.value
                            else {
                                return Err(format!(
                                    "{}: cron `{name}` must be a string literal",
                                    file.display()
                                ));
                            };
                            if name == "schedule" {
                                if schedule.replace(value.value()).is_some() {
                                    return Err(format!(
                                        "{}: duplicate cron schedule",
                                        file.display()
                                    ));
                                }
                                continue;
                            }
                            provider = match value.value().as_str() {
                                "vercel" => Provider::Vercel,
                                "cloudflare" => Provider::Cloudflare,
                                other => {
                                    return Err(format!(
                                        "{}: unknown cron provider `{other}`",
                                        file.display()
                                    ));
                                }
                            }
                        }
                        "disabled" => {
                            let syn::Expr::Lit(syn::ExprLit {
                                lit: syn::Lit::Bool(value),
                                ..
                            }) = arg.value
                            else {
                                return Err(format!(
                                    "{}: cron `disabled` must be a boolean literal",
                                    file.display()
                                ));
                            };
                            if disabled.replace(value.value).is_some() {
                                return Err(format!("{}: duplicate cron disabled", file.display()));
                            }
                        }
                        other => {
                            return Err(format!(
                                "{}: unknown cron option `{other}`",
                                file.display()
                            ));
                        }
                    }
                }
                let schedule = schedule.ok_or_else(|| {
                    format!(
                        "{}: #[nextrs::cron] requires `schedule = \"...\"`",
                        file.display()
                    )
                })?;
                let fields = schedule.split_whitespace().count();
                if fields != 5 {
                    return Err(format!(
                        "{}: cron schedule `{schedule}` must have 5 fields, found {fields}",
                        file.display()
                    ));
                }
                if disabled == Some(true) {
                    continue;
                }
                let path = route_path(&app, &file)?;
                crons.push(CronEntry {
                    path,
                    schedule,
                    provider,
                    source: file.clone(),
                });
            }
        }
    }
    crons.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(crons)
}

fn find_named_files(dir: &Path, name: &str, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_named_files(&path, name, found);
        } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            found.push(path);
        }
    }
}

fn route_path(app: &Path, file: &Path) -> Result<String, String> {
    let relative = file
        .parent()
        .and_then(|parent| parent.strip_prefix(app).ok())
        .ok_or_else(|| format!("{} is not under {}", file.display(), app.display()))?;
    let mut segments = Vec::new();
    for segment in relative.iter().filter_map(|part| part.to_str()) {
        if segment.starts_with('(') && segment.ends_with(')') {
            continue;
        }
        if segment.starts_with('[') {
            return Err(format!(
                "{}: scheduled cron routes cannot contain dynamic path segment `{segment}`",
                file.display()
            ));
        }
        segments.push(segment);
    }
    Ok(if segments.is_empty() {
        "/".into()
    } else {
        format!("/{}", segments.join("/"))
    })
}

fn warn_subdaily_vercel(crons: &[CronEntry]) {
    for cron in crons
        .iter()
        .filter(|cron| cron.provider == Provider::Vercel && !is_daily_or_coarser(&cron.schedule))
    {
        eprintln!(
            "nextrs: warning: {} uses subdaily Vercel schedule `{}`; Vercel Hobby supports only daily crons. If this is a Hobby project, add `provider = \"cloudflare\"` to #[nextrs::cron] and configure CRON_SECRET for flexible free scheduling. See https://nextrs.hirschi.dev/docs/crons",
            cron.path, cron.schedule
        );
    }
}

/// Validate external-provider credentials before a potentially expensive
/// Vercel build. Returns the secret when Cloudflare cron routes exist.
pub fn preflight_cloudflare_credentials(
    crons: &[&CronEntry],
    env_files: &crate::env_file::Report,
) -> Result<Option<String>, String> {
    if crons.is_empty() {
        return Ok(None);
    }
    let secret = std::env::var("CRON_SECRET")
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "Cloudflare cron routes are configured, but CRON_SECRET is not set.{}\n  It must match the value on the Vercel app. Pull it with:\n    vercel env pull .vercel/.env.production.local --environment=production\n  or set [deploy] env_file in nextrs.toml.\n  See https://nextrs.hirschi.dev/docs/config#deploy-env-files",
                env_files.describe_missing("CRON_SECRET")
            )
        })?;
    let token = std::env::var("CLOUDFLARE_API_TOKEN")
        .ok()
        .filter(|value| !value.is_empty());
    let account = std::env::var("CLOUDFLARE_ACCOUNT_ID")
        .ok()
        .filter(|value| !value.is_empty());
    if token.is_some() != account.is_some() {
        return Err("set both CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID for API deployment, or neither to use wrangler".into());
    }
    Ok(Some(secret))
}

/// Generate all cron plumbing. Returns a human summary of what was written.
pub fn generate(root: &Path) -> Result<String, String> {
    let config = load_config(root)?;
    if let Some(warning) = legacy_vercel_warning(root) {
        eprintln!("{warning}");
    }
    let crons = discover_crons(root)?;
    warn_subdaily_vercel(&crons);
    let cloudflare: Vec<&CronEntry> = crons
        .iter()
        .filter(|cron| cron.provider() == Provider::Cloudflare)
        .collect();
    let vercel: Vec<&CronEntry> = crons
        .iter()
        .filter(|cron| cron.provider() == Provider::Vercel)
        .collect();

    let mut summary = Vec::new();

    fs::create_dir_all(root.join(".nextrs"))
        .map_err(|error| format!("failed to create {}/.nextrs: {error}", root.display()))?;
    let settings = config.vercel.as_ref().cloned().unwrap_or_default();
    let vercel_json = root.join(VERCEL_CONFIG_FILE);
    let mut json = render_vercel_json(&settings, &vercel)?;
    let plan = crate::bundles::plan(root)?;
    if plan.enabled {
        json["buildCommand"] = json!(
            "echo 'Server bundles require nextrs deploy or nextrs bundles build --vercel followed by vercel deploy --prebuilt' >&2; exit 1"
        );
    }
    write(
        &root.join(".nextrs/bundle-manifest.json"),
        &serde_json::to_string_pretty(&plan).unwrap(),
    )?;
    write(
        &vercel_json,
        &format!("{}\n", serde_json::to_string_pretty(&json).unwrap()),
    )?;
    write(
        &root.join(GENERATED_README),
        "# Generated NextRS state\n\nFiles in this directory are managed by NextRS. Do not edit `vercel.json` directly; configure deployment in `nextrs.toml` and cron schedules with `#[nextrs::cron(...)]`.\n",
    )?;
    summary.push(format!(
        "{VERCEL_CONFIG_FILE}: generated with {} cron(s)",
        vercel.len()
    ));

    let out_dir = root.join(OUTPUT_DIR);
    if cloudflare.is_empty() {
        // Stale worker config would still deploy on `cron deploy`; remove it.
        if out_dir.exists() {
            fs::remove_dir_all(&out_dir)
                .map_err(|error| format!("failed to remove {}: {error}", out_dir.display()))?;
        }
    } else {
        fs::create_dir_all(&out_dir)
            .map_err(|error| format!("failed to create {}: {error}", out_dir.display()))?;
        write(&out_dir.join("worker.js"), &worker_js(&cloudflare))?;
        write(
            &out_dir.join("wrangler.toml"),
            &wrangler_toml(&config.app, &cloudflare),
        )?;
        summary.push(format!(
            "{OUTPUT_DIR}: worker with {} cron(s)",
            cloudflare.len()
        ));
    }

    Ok(summary.join(", "))
}

fn legacy_vercel_warning(root: &Path) -> Option<String> {
    let legacy = root.join("vercel.json");
    legacy.is_file().then(|| {
        format!(
            "nextrs: warning: {} is ignored because NextRS generates {}. Copy supported deployment settings into the [vercel] table in {}; put other non-framework Vercel settings under [vercel.extra]. See https://nextrs.hirschi.dev/docs/config",
            legacy.display(),
            root.join(VERCEL_CONFIG_FILE).display(),
            root.join(CONFIG_FILE).display(),
        )
    })
}

/// Deploy the generated Worker and its `CRON_SECRET`.
///
/// Two transports, chosen by environment:
/// - `CLOUDFLARE_API_TOKEN` + `CLOUDFLARE_ACCOUNT_ID` set → talk to the
///   Cloudflare API directly (no wrangler or Node needed; the CI-friendly
///   path). The secret ships as a binding in the same upload.
/// - otherwise → shell out to `wrangler`, which brings its own login.
pub fn deploy(root: &Path) -> Result<(), String> {
    let summary = generate(root)?;
    eprintln!("nextrs: generated {summary}");

    let config = load_config(root)?;
    let crons = discover_crons(root)?;
    let cloudflare: Vec<&CronEntry> = crons
        .iter()
        .filter(|cron| cron.provider() == Provider::Cloudflare)
        .collect();
    if cloudflare.is_empty() {
        eprintln!("nextrs: no cloudflare crons declared; nothing to deploy");
        return Ok(());
    }

    preflight(root, &cloudflare)?;

    let env_files = crate::env_file::load(
        root,
        None,
        crate::env_file::Target::Production,
        config.deploy.as_ref(),
    )?;
    let secret = preflight_cloudflare_credentials(&cloudflare, &env_files)?
        .expect("cloudflare crons exist");

    let api = (
        std::env::var("CLOUDFLARE_API_TOKEN")
            .ok()
            .filter(|v| !v.is_empty()),
        std::env::var("CLOUDFLARE_ACCOUNT_ID")
            .ok()
            .filter(|v| !v.is_empty()),
    );
    match api {
        (Some(token), Some(account)) => {
            let auth = CloudflareApi { token, account };
            let script = fs::read_to_string(root.join(OUTPUT_DIR).join("worker.js"))
                .map_err(|error| format!("failed to read generated worker.js: {error}"))?;
            deploy_via_api(&auth, &config.app, &cloudflare, &script, &secret)?;
            eprintln!("nextrs: cloudflare cron worker deployed (Cloudflare API)");
        }
        (Some(_), None) | (None, Some(_)) => {
            return Err("set both CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID for an API-direct deploy, or neither to use wrangler".into());
        }
        (None, None) => {
            // Absolute: run_wrangler runs with the app root as cwd, so a
            // cwd-relative root would otherwise be joined twice.
            let wrangler_config = fs::canonicalize(root.join(OUTPUT_DIR).join("wrangler.toml"))
                .map_err(|error| format!("generated wrangler.toml missing: {error}"))?;
            run_wrangler(root, &wrangler_config, &["deploy"], None)?;
            run_wrangler(
                root,
                &wrangler_config,
                &["secret", "put", "CRON_SECRET"],
                Some(&secret),
            )?;
            eprintln!("nextrs: cloudflare cron worker deployed (wrangler)");
        }
    }
    Ok(())
}

struct CloudflareApi {
    token: String,
    account: String,
}

const CLOUDFLARE_API: &str = "https://api.cloudflare.com/client/v4";

/// Upload the Worker module with its bindings, then set its cron triggers —
/// the same two calls wrangler makes, minus wrangler.
fn deploy_via_api(
    api: &CloudflareApi,
    app: &AppConfig,
    crons: &[&CronEntry],
    script: &str,
    secret: &str,
) -> Result<(), String> {
    let name = worker_name(app);
    let base = format!(
        "{CLOUDFLARE_API}/accounts/{}/workers/scripts/{name}",
        api.account
    );

    let metadata = json!({
        "main_module": "worker.js",
        "compatibility_date": COMPATIBILITY_DATE,
        "bindings": [
            { "type": "plain_text", "name": "APP_URL", "text": app.url.trim_end_matches('/') },
            { "type": "secret_text", "name": "CRON_SECRET", "text": secret },
        ],
    });
    let (content_type, body) = multipart(&[
        (
            "metadata",
            None,
            "application/json",
            metadata.to_string().as_bytes(),
        ),
        (
            "worker.js",
            Some("worker.js"),
            "application/javascript+module",
            script.as_bytes(),
        ),
    ]);
    cloudflare_call(
        ureq::put(&base)
            .set("Authorization", &format!("Bearer {}", api.token))
            .set("Content-Type", &content_type),
        body,
        "upload worker",
    )?;
    eprintln!("nextrs: uploaded worker {name}");

    let mut schedules: Vec<&str> = crons.iter().map(|cron| cron.schedule.as_str()).collect();
    schedules.sort_unstable();
    schedules.dedup();
    let triggers: Vec<Value> = schedules
        .iter()
        .map(|cron| json!({ "cron": cron }))
        .collect();
    cloudflare_call(
        ureq::put(&format!("{base}/schedules"))
            .set("Authorization", &format!("Bearer {}", api.token))
            .set("Content-Type", "application/json"),
        Value::Array(triggers).to_string().into_bytes(),
        "set schedules",
    )?;
    eprintln!("nextrs: set {} schedule(s) on {name}", schedules.len());
    Ok(())
}

fn cloudflare_call(request: ureq::Request, body: Vec<u8>, what: &str) -> Result<(), String> {
    let response = match request.send_bytes(&body) {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let text = response.into_string().unwrap_or_default();
            return Err(format!(
                "cloudflare: {what} failed with HTTP {code}: {}",
                cloudflare_errors(&text)
            ));
        }
        Err(error) => return Err(format!("cloudflare: {what} failed: {error}")),
    };
    let text = response
        .into_string()
        .map_err(|error| format!("cloudflare: {what}: unreadable response: {error}"))?;
    let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if json["success"] == json!(true) {
        Ok(())
    } else {
        Err(format!(
            "cloudflare: {what} failed: {}",
            cloudflare_errors(&text)
        ))
    }
}

/// Flatten the API's `errors: [{code, message}]` for a one-line diagnostic.
fn cloudflare_errors(text: &str) -> String {
    let json: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let errors: Vec<String> = json["errors"]
        .as_array()
        .map(|errors| {
            errors
                .iter()
                .map(|e| format!("{} ({})", e["message"].as_str().unwrap_or("?"), e["code"]))
                .collect()
        })
        .unwrap_or_default();
    if errors.is_empty() {
        text.chars().take(300).collect()
    } else {
        errors.join("; ")
    }
}

/// Encode `multipart/form-data`; returns (Content-Type, body).
fn multipart(parts: &[(&str, Option<&str>, &str, &[u8])]) -> (String, Vec<u8>) {
    let boundary = format!("nextrs-{}", std::process::id());
    let mut body = Vec::new();
    for (name, filename, content_type, data) in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let disposition = match filename {
            Some(filename) => format!("form-data; name=\"{name}\"; filename=\"{filename}\""),
            None => format!("form-data; name=\"{name}\""),
        };
        body.extend_from_slice(
            format!("Content-Disposition: {disposition}\r\nContent-Type: {content_type}\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn worker_name(app: &AppConfig) -> String {
    format!("{}-cron", app.name)
}

/// Before deploying the trigger, verify the target actually looks like a
/// deployed nextrs app with fail-closed cron routes: an unauthenticated GET
/// of each cloudflare-provider path should return 401. Anything else means
/// the setup is wrong in a way worth flagging — the Worker would either hit
/// a dead URL every tick or an unprotected route.
fn preflight(root: &Path, crons: &[&CronEntry]) -> Result<(), String> {
    let config = load_config(root)?;
    let base = config.app.url.trim_end_matches('/');
    let mut problems = Vec::new();
    for cron in crons {
        let url = format!("{base}{}", cron.path);
        match ureq::get(&url).timeout(std::time::Duration::from_secs(10)).call() {
            Err(ureq::Error::Status(401, _)) => {
                eprintln!("nextrs: preflight ok: {url} answers 401 without the secret");
            }
            Ok(response) => problems.push(format!(
                "{url} answered {} WITHOUT authentication — the deployed route is missing #[nextrs::cron] or is not the route you meant",
                response.status()
            )),
            Err(ureq::Error::Status(404, _)) => problems.push(format!(
                "{url} answered 404 — the route isn't deployed at app.url (stale deploy or wrong app.url)"
            )),
            Err(ureq::Error::Status(code, _)) => problems.push(format!(
                "{url} answered {code} without authentication (expected 401)"
            )),
            Err(error) => problems.push(format!(
                "{url} is unreachable: {error} — check `app.url` in nextrs.toml and that the app is deployed"
            )),
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    for problem in &problems {
        eprintln!("nextrs: preflight warning: {problem}");
    }
    if std::env::var_os("NEXTRS_CRON_SKIP_PREFLIGHT").is_some() {
        eprintln!("nextrs: NEXTRS_CRON_SKIP_PREFLIGHT set; deploying anyway");
        return Ok(());
    }
    Err(
        "cron preflight failed: the deployed app doesn't look ready for these triggers (see warnings above). Fix the cron route or deployment, or set NEXTRS_CRON_SKIP_PREFLIGHT=1 to deploy anyway"
            .into(),
    )
}

fn run_wrangler(
    root: &Path,
    config: &Path,
    args: &[&str],
    stdin: Option<&str>,
) -> Result<(), String> {
    let mut command = Command::new("wrangler");
    command
        .current_dir(root)
        .args(args)
        .arg("--config")
        .arg(config);
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().map_err(|error| {
        format!("failed to run wrangler (is it installed? `npm i -g wrangler`): {error}")
    })?;
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .expect("stdin was piped")
            .write_all(input.as_bytes())
            .map_err(|error| format!("failed to write to wrangler stdin: {error}"))?;
    }
    let status = child
        .wait()
        .map_err(|error| format!("failed to wait for wrangler: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("wrangler {} failed with {status}", args.join(" ")))
    }
}

/// The complete `vercel.json` for a nextrs app, from the `[vercel]` table.
fn render_vercel_json(settings: &VercelConfig, crons: &[&CronEntry]) -> Result<Value, String> {
    let mut json = serde_json::Map::new();
    json.insert(
        "$schema".into(),
        json!("https://openapi.vercel.sh/vercel.json"),
    );
    if !settings.regions.is_empty() {
        json.insert("regions".into(), json!(settings.regions));
    }
    json.insert(
        "installCommand".into(),
        json!(settings.install_command.as_deref().unwrap_or("npm ci")),
    );
    json.insert(
        "buildCommand".into(),
        json!(
            settings
                .build_command
                .as_deref()
                .unwrap_or(DEFAULT_BUILD_COMMAND)
        ),
    );
    json.insert(
        "functions".into(),
        json!({ "api/index.rs": { "runtime": settings.runtime.as_deref().unwrap_or(DEFAULT_VERCEL_RUNTIME) } }),
    );
    json.insert(
        "headers".into(),
        json!([{
            "source": "/dist/(.*)",
            "headers": [{ "key": "Cache-Control", "value": "public, max-age=31536000, immutable" }]
        }]),
    );
    json.insert(
        "rewrites".into(),
        json!([{ "source": "/(.*)", "destination": "/api/index" }]),
    );
    json.insert(
        "git".into(),
        json!({ "deploymentEnabled": settings.git_deploys.unwrap_or(false) }),
    );
    if !crons.is_empty() {
        let entries: Vec<Value> = crons
            .iter()
            .map(|cron| json!({ "path": cron.path, "schedule": cron.schedule }))
            .collect();
        json.insert("crons".into(), Value::Array(entries));
    }
    for (key, value) in &settings.extra {
        if matches!(
            key.as_str(),
            "$schema"
                | "regions"
                | "installCommand"
                | "buildCommand"
                | "functions"
                | "headers"
                | "rewrites"
                | "git"
                | "crons"
        ) {
            return Err(format!(
                "[vercel.extra] `{key}` is managed by NextRS and cannot be overridden"
            ));
        }
        let value = serde_json::to_value(value)
            .map_err(|error| format!("[vercel.extra] {key}: {error}"))?;
        json.insert(key.clone(), value);
    }
    Ok(Value::Object(json))
}

fn worker_js(crons: &[&CronEntry]) -> String {
    let mut routes = serde_json::Map::new();
    for cron in crons {
        routes
            .entry(cron.schedule.clone())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .unwrap()
            .push(Value::String(cron.path.clone()));
    }
    let routes = serde_json::to_string_pretty(&Value::Object(routes)).unwrap();
    format!(
        r#"// Generated by `nextrs cron generate` from #[nextrs::cron] routes — do not edit.
// This Worker is a trigger, not a runtime: it only fetches the app's cron
// routes on Vercel. Delete it and you lose nothing but the schedule.
const ROUTES = {routes};

export default {{
  async scheduled(event, env, ctx) {{
    const paths = ROUTES[event.cron] ?? [];
    ctx.waitUntil(Promise.all(paths.map(async (path) => {{
      const response = await fetch(env.APP_URL + path, {{
        headers: {{ Authorization: `Bearer ${{env.CRON_SECRET}}` }},
      }});
      console.log(`${{event.cron}} ${{path}} -> ${{response.status}}`);
    }})));
  }},
}};
"#
    )
}

fn wrangler_toml(app: &AppConfig, crons: &[&CronEntry]) -> String {
    let mut schedules: Vec<&str> = crons.iter().map(|cron| cron.schedule.as_str()).collect();
    schedules.sort_unstable();
    schedules.dedup();
    let schedules = schedules
        .iter()
        .map(|schedule| format!("{schedule:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"# Generated by `nextrs cron generate` from #[nextrs::cron] routes — do not edit.
name = "{name}"
main = "worker.js"
compatibility_date = "{COMPATIBILITY_DATE}"

[vars]
APP_URL = "{url}"

[triggers]
crons = [{schedules}]
"#,
        name = worker_name(app),
        url = app.url.trim_end_matches('/'),
    )
}

fn write(path: &Path, content: &str) -> Result<(), String> {
    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
    ));
    fs::write(&temporary, content)
        .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("failed to replace {}: {error}", path.display()))
}

pub fn resolve_root(root: Option<PathBuf>) -> Result<PathBuf, String> {
    match root {
        Some(root) => Ok(root),
        None => std::env::current_dir().map_err(|error| format!("cannot resolve cwd: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
[app]
name = "demo"
url = "https://demo.vercel.app/"
"#;

    fn setup(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(CONFIG_FILE), CONFIG).unwrap();
        for (path, attr) in [
            (
                "sweep",
                r#"schedule = "*/5 * * * *", provider = "cloudflare""#,
            ),
            ("digest", r#"schedule = "0 6 * * *""#),
            (
                "forced",
                r#"schedule = "0 7 * * *", provider = "cloudflare""#,
            ),
            ("disabled", r#"schedule = "0 8 * * *", disabled = true"#),
        ] {
            let route = dir.join("app/api/cron").join(path).join("route.rs");
            fs::create_dir_all(route.parent().unwrap()).unwrap();
            fs::write(
                route,
                format!("#[nextrs::cron({attr})]\npub async fn get() {{}}\n"),
            )
            .unwrap();
        }
    }

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nextrs-cron-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn discovers_macro_schedules_and_providers() {
        let dir = tempdir("providers");
        setup(&dir);
        let crons = discover_crons(&dir).unwrap();
        assert_eq!(
            crons
                .iter()
                .find(|c| c.path.ends_with("sweep"))
                .unwrap()
                .provider(),
            Provider::Cloudflare
        );
        assert_eq!(
            crons
                .iter()
                .find(|c| c.path.ends_with("digest"))
                .unwrap()
                .provider(),
            Provider::Vercel
        );
        assert_eq!(
            crons
                .iter()
                .find(|c| c.path.ends_with("forced"))
                .unwrap()
                .provider(),
            Provider::Cloudflare
        );
        assert!(!crons.iter().any(|cron| cron.path.ends_with("disabled")));
    }

    #[test]
    fn generate_writes_worker_wrangler_and_vercel_json() {
        let dir = tempdir("generate");
        setup(&dir);

        generate(&dir).unwrap();

        let worker = fs::read_to_string(dir.join(OUTPUT_DIR).join("worker.js")).unwrap();
        assert!(worker.contains(r#""*/5 * * * *""#));
        assert!(worker.contains("/api/cron/sweep"));
        assert!(worker.contains("/api/cron/forced"));
        assert!(!worker.contains("/api/cron/digest"));

        let wrangler = fs::read_to_string(dir.join(OUTPUT_DIR).join("wrangler.toml")).unwrap();
        assert!(wrangler.contains(r#"name = "demo-cron""#));
        // trailing slash trimmed from app.url
        assert!(wrangler.contains(r#"APP_URL = "https://demo.vercel.app""#));
        assert!(wrangler.contains(r#"crons = ["*/5 * * * *", "0 7 * * *"]"#));

        let vercel: Value =
            serde_json::from_str(&fs::read_to_string(dir.join(VERCEL_CONFIG_FILE)).unwrap())
                .unwrap();
        assert_eq!(vercel["git"]["deploymentEnabled"], json!(false));
        assert_eq!(
            vercel["crons"],
            json!([{ "path": "/api/cron/digest", "schedule": "0 6 * * *" }])
        );
        assert!(
            fs::read_to_string(dir.join(GENERATED_README))
                .unwrap()
                .contains("managed by NextRS")
        );
    }

    #[test]
    fn vercel_table_generates_whole_vercel_json() {
        let dir = tempdir("vercel-table");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(CONFIG_FILE),
            r#"
[app]
name = "demo"
url = "https://demo.vercel.app"

[vercel]
regions = ["pdx1"]

[vercel.extra]
trailingSlash = false
"#,
        )
        .unwrap();
        let route = dir.join("app/api/cron/digest/route.rs");
        fs::create_dir_all(route.parent().unwrap()).unwrap();
        fs::write(
            route,
            "#[nextrs::cron(schedule = \"0 6 * * *\")]\npub async fn get() {}\n",
        )
        .unwrap();
        // A hand-written root file is neither read nor changed.
        fs::write(dir.join("vercel.json"), r#"{ "stale": true }"#).unwrap();

        generate(&dir).unwrap();

        let vercel: Value =
            serde_json::from_str(&fs::read_to_string(dir.join(VERCEL_CONFIG_FILE)).unwrap())
                .unwrap();
        assert!(vercel.get("stale").is_none());
        assert_eq!(
            fs::read_to_string(dir.join("vercel.json")).unwrap(),
            r#"{ "stale": true }"#
        );
        assert_eq!(vercel["regions"], json!(["pdx1"]));
        assert_eq!(vercel["installCommand"], json!("npm ci"));
        assert_eq!(vercel["buildCommand"], json!(DEFAULT_BUILD_COMMAND));
        assert_eq!(
            vercel["functions"]["api/index.rs"]["runtime"],
            json!(DEFAULT_VERCEL_RUNTIME)
        );
        assert_eq!(vercel["rewrites"][0]["destination"], json!("/api/index"));
        assert_eq!(vercel["git"]["deploymentEnabled"], json!(false));
        assert_eq!(vercel["trailingSlash"], json!(false));
        assert_eq!(
            vercel["crons"],
            json!([{ "path": "/api/cron/digest", "schedule": "0 6 * * *" }])
        );
    }

    #[test]
    fn vercel_extra_rejects_framework_owned_keys() {
        for key in [
            "$schema",
            "regions",
            "installCommand",
            "buildCommand",
            "functions",
            "headers",
            "rewrites",
            "git",
            "crons",
        ] {
            let source = format!("[extra]\n\"{key}\" = []\n");
            let settings: VercelConfig = toml::from_str(&source).unwrap();
            let error = render_vercel_json(&settings, &[]).unwrap_err();
            assert!(error.contains("managed by NextRS"), "{key}: {error}");
        }
    }

    #[test]
    fn legacy_root_vercel_json_warns_with_migration_action() {
        let dir = tempdir("legacy-vercel-warning");
        fs::create_dir_all(&dir).unwrap();
        assert!(legacy_vercel_warning(&dir).is_none());

        fs::write(dir.join("vercel.json"), "{}").unwrap();
        let warning = legacy_vercel_warning(&dir).unwrap();
        assert!(warning.contains("vercel.json is ignored"));
        assert!(warning.contains("Copy supported deployment settings"));
        assert!(warning.contains(CONFIG_FILE));
        assert!(warning.contains("[vercel] table"));
        assert!(warning.contains("[vercel.extra]"));
        assert!(!warning.contains("delete"));
    }

    #[test]
    fn generate_removes_stale_worker_when_no_cloudflare_crons() {
        let dir = tempdir("stale");
        setup(&dir);
        generate(&dir).unwrap();
        assert!(dir.join(OUTPUT_DIR).join("worker.js").is_file());

        fs::remove_file(dir.join("app/api/cron/sweep/route.rs")).unwrap();
        fs::remove_file(dir.join("app/api/cron/forced/route.rs")).unwrap();
        generate(&dir).unwrap();
        assert!(!dir.join(OUTPUT_DIR).exists());
    }

    #[test]
    fn multipart_encodes_parts_with_boundary() {
        let (content_type, body) = multipart(&[
            ("metadata", None, "application/json", b"{}"),
            (
                "worker.js",
                Some("worker.js"),
                "application/javascript+module",
                b"export default {}",
            ),
        ]);
        let boundary = content_type
            .strip_prefix("multipart/form-data; boundary=")
            .unwrap();
        let body = String::from_utf8(body).unwrap();
        assert!(body.starts_with(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"metadata\"\r\nContent-Type: application/json\r\n\r\n{{}}\r\n")));
        assert!(body.contains("name=\"worker.js\"; filename=\"worker.js\"\r\nContent-Type: application/javascript+module\r\n\r\nexport default {}\r\n"));
        assert!(body.ends_with(&format!("--{boundary}--\r\n")));
    }

    #[test]
    fn rejects_bad_schedule_and_url() {
        let dir = tempdir("invalid-schedule");
        fs::create_dir_all(dir.join("app/api/cron/x")).unwrap();
        fs::write(
            dir.join(CONFIG_FILE),
            "[app]\nname = \"a\"\nurl = \"https://a.dev\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("app/api/cron/x/route.rs"),
            "#[nextrs::cron(schedule = \"* * * *\")]\npub async fn get() {}\n",
        )
        .unwrap();
        assert!(
            discover_crons(&dir)
                .unwrap_err()
                .contains("must have 5 fields")
        );

        fs::write(
            dir.join("app/api/cron/x/route.rs"),
            "#[nextrs::cron(schedule = \"0 6 * * *\", disabled = \"yes\")]\npub async fn get() {}\n",
        )
        .unwrap();
        assert!(
            discover_crons(&dir)
                .unwrap_err()
                .contains("`disabled` must be a boolean literal")
        );

        fs::write(
            dir.join(CONFIG_FILE),
            "[app]\nname = \"a\"\nurl = \"a.dev\"\n",
        )
        .unwrap();
        assert!(
            load_config(&dir)
                .unwrap_err()
                .contains("absolute http(s) URL")
        );
    }
}
