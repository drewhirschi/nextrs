//! Cron declaration, codegen, and deploy.
//!
//! Apps declare schedules once in `nextrs.toml`; the CLI turns that into the
//! provider plumbing:
//!
//! - `cloudflare` crons become a generated Worker (`.nextrs/cloudflare/`)
//!   whose `scheduled()` handler does nothing but fetch the app's route on
//!   Vercel with `Authorization: Bearer $CRON_SECRET`. All real logic stays
//!   in the Rust app; the Worker is disposable plumbing.
//! - `vercel` crons are merged into the app's `vercel.json` `crons` array.
//!
//! Provider defaults route coarse (daily-or-slower) schedules to native
//! Vercel crons and anything finer to the Cloudflare shim, because Vercel
//! Hobby allows only one imprecise cron per day while Cloudflare's free tier
//! handles minutely schedules.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::{Value, json};

pub const CONFIG_FILE: &str = "nextrs.toml";
pub const OUTPUT_DIR: &str = ".nextrs/cloudflare";

/// Wrangler pins Worker runtime behavior to this date; bump deliberately.
const COMPATIBILITY_DATE: &str = "2026-08-01";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextrsConfig {
    pub app: AppConfig,
    #[serde(default)]
    pub crons: Vec<CronEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// Used to name the generated Worker (`<name>-cron`).
    pub name: String,
    /// The deployed app's base URL, e.g. `https://myapp.vercel.app`.
    pub url: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CronEntry {
    /// Route path on the app, e.g. `/api/cron/refresh`.
    pub path: String,
    /// Five-field cron expression.
    pub schedule: String,
    /// `cloudflare` | `vercel`; defaults by schedule granularity.
    pub provider: Option<Provider>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Cloudflare,
    Vercel,
}

impl CronEntry {
    pub fn provider(&self) -> Provider {
        self.provider.unwrap_or({
            if is_daily_or_coarser(&self.schedule) {
                Provider::Vercel
            } else {
                Provider::Cloudflare
            }
        })
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
    for cron in &config.crons {
        if !cron.path.starts_with('/') {
            return Err(format!(
                "{}: cron path `{}` must start with `/`",
                path.display(),
                cron.path
            ));
        }
        let fields = cron.schedule.split_whitespace().count();
        if fields != 5 {
            return Err(format!(
                "{}: cron schedule `{}` must have 5 fields, found {fields}",
                path.display(),
                cron.schedule
            ));
        }
    }
    if !config.app.url.starts_with("https://") && !config.app.url.starts_with("http://") {
        return Err(format!(
            "{}: app.url `{}` must be an absolute http(s) URL",
            path.display(),
            config.app.url
        ));
    }
    Ok(config)
}

/// Generate all cron plumbing. Returns a human summary of what was written.
pub fn generate(root: &Path) -> Result<String, String> {
    let config = load_config(root)?;
    let cloudflare: Vec<&CronEntry> = config
        .crons
        .iter()
        .filter(|cron| cron.provider() == Provider::Cloudflare)
        .collect();
    let vercel: Vec<&CronEntry> = config
        .crons
        .iter()
        .filter(|cron| cron.provider() == Provider::Vercel)
        .collect();

    let mut summary = Vec::new();

    if !vercel.is_empty() {
        merge_vercel_crons(&root.join("vercel.json"), &vercel)?;
        summary.push(format!("vercel.json: {} cron(s)", vercel.len()));
    }

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
        summary.push(format!("{OUTPUT_DIR}: worker with {} cron(s)", cloudflare.len()));
    }

    Ok(summary.join(", "))
}

/// Deploy the generated Worker with wrangler and sync `CRON_SECRET`.
pub fn deploy(root: &Path) -> Result<(), String> {
    let summary = generate(root)?;
    eprintln!("nextrs: generated {summary}");

    // Absolute: run_wrangler runs with the app root as cwd, so a cwd-relative
    // root would otherwise be joined twice.
    let wrangler_config = fs::canonicalize(root.join(OUTPUT_DIR).join("wrangler.toml"));
    let Ok(wrangler_config) = wrangler_config else {
        eprintln!("nextrs: no cloudflare crons declared; nothing to deploy");
        return Ok(());
    };

    preflight(root)?;

    let secret = std::env::var("CRON_SECRET")
        .ok()
        .filter(|secret| !secret.is_empty())
        .ok_or("CRON_SECRET must be set in the environment to deploy (the Worker sends it, the app verifies it)")?;

    run_wrangler(root, &wrangler_config, &["deploy"], None)?;
    run_wrangler(
        root,
        &wrangler_config,
        &["secret", "put", "CRON_SECRET"],
        Some(&secret),
    )?;
    eprintln!("nextrs: cloudflare cron worker deployed");
    Ok(())
}

/// Before deploying the trigger, verify the target actually looks like a
/// deployed nextrs app with fail-closed cron routes: an unauthenticated GET
/// of each cloudflare-provider path should return 401. Anything else means
/// the setup is wrong in a way worth flagging — the Worker would either hit
/// a dead URL every tick or an unprotected route.
fn preflight(root: &Path) -> Result<(), String> {
    let config = load_config(root)?;
    let base = config.app.url.trim_end_matches('/');
    let mut problems = Vec::new();
    for cron in config.crons.iter().filter(|c| c.provider() == Provider::Cloudflare) {
        let url = format!("{base}{}", cron.path);
        match ureq::get(&url).timeout(std::time::Duration::from_secs(10)).call() {
            Err(ureq::Error::Status(401, _)) => {
                eprintln!("nextrs: preflight ok: {url} answers 401 without the secret");
            }
            Ok(response) => problems.push(format!(
                "{url} answered {} WITHOUT authentication — the route is missing its `nextrs::cron::authorize` gate or is not the route you meant",
                response.status()
            )),
            Err(ureq::Error::Status(404, _)) => problems.push(format!(
                "{url} answered 404 — the route isn't deployed at app.url (stale deploy, or wrong `app.url`/`path` in nextrs.toml)"
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
        "cron preflight failed: the deployed app doesn't look ready for these triggers (see warnings above). Fix nextrs.toml or the deployment, or set NEXTRS_CRON_SKIP_PREFLIGHT=1 to deploy anyway"
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

/// Replace the `crons` array in `vercel.json`, preserving every other key.
fn merge_vercel_crons(path: &Path, crons: &[&CronEntry]) -> Result<(), String> {
    let mut json: Value = if path.is_file() {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?
    } else {
        json!({})
    };
    let entries: Vec<Value> = crons
        .iter()
        .map(|cron| json!({ "path": cron.path, "schedule": cron.schedule }))
        .collect();
    json.as_object_mut()
        .ok_or_else(|| format!("{} must contain a JSON object", path.display()))?
        .insert("crons".to_string(), Value::Array(entries));
    write(path, &format!("{}\n", serde_json::to_string_pretty(&json).unwrap()))
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
        r#"// Generated by `nextrs cron generate` from nextrs.toml — do not edit.
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
        r#"# Generated by `nextrs cron generate` from nextrs.toml — do not edit.
name = "{name}-cron"
main = "worker.js"
compatibility_date = "{COMPATIBILITY_DATE}"

[vars]
APP_URL = "{url}"

[triggers]
crons = [{schedules}]
"#,
        name = app.name,
        url = app.url.trim_end_matches('/'),
    )
}

fn write(path: &Path, content: &str) -> Result<(), String> {
    fs::write(path, content).map_err(|error| format!("failed to write {}: {error}", path.display()))
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

[[crons]]
path = "/api/cron/sweep"
schedule = "*/5 * * * *"

[[crons]]
path = "/api/cron/digest"
schedule = "0 6 * * *"

[[crons]]
path = "/api/cron/forced"
schedule = "0 7 * * *"
provider = "cloudflare"
"#;

    fn setup(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(CONFIG_FILE), CONFIG).unwrap();
    }

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nextrs-cron-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn provider_defaults_by_granularity() {
        let dir = tempdir("providers");
        setup(&dir);
        let config = load_config(&dir).unwrap();
        assert_eq!(config.crons[0].provider(), Provider::Cloudflare); // */5 minutes
        assert_eq!(config.crons[1].provider(), Provider::Vercel); // daily
        assert_eq!(config.crons[2].provider(), Provider::Cloudflare); // forced
    }

    #[test]
    fn generate_writes_worker_wrangler_and_vercel_json() {
        let dir = tempdir("generate");
        setup(&dir);
        fs::write(
            dir.join("vercel.json"),
            r#"{ "git": { "deploymentEnabled": false } }"#,
        )
        .unwrap();

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
            serde_json::from_str(&fs::read_to_string(dir.join("vercel.json")).unwrap()).unwrap();
        assert_eq!(vercel["git"]["deploymentEnabled"], json!(false));
        assert_eq!(
            vercel["crons"],
            json!([{ "path": "/api/cron/digest", "schedule": "0 6 * * *" }])
        );
    }

    #[test]
    fn generate_removes_stale_worker_when_no_cloudflare_crons() {
        let dir = tempdir("stale");
        setup(&dir);
        generate(&dir).unwrap();
        assert!(dir.join(OUTPUT_DIR).join("worker.js").is_file());

        fs::write(
            dir.join(CONFIG_FILE),
            "[app]\nname = \"demo\"\nurl = \"https://demo.vercel.app\"\n\n[[crons]]\npath = \"/api/cron/digest\"\nschedule = \"0 6 * * *\"\n",
        )
        .unwrap();
        generate(&dir).unwrap();
        assert!(!dir.join(OUTPUT_DIR).exists());
    }

    #[test]
    fn rejects_bad_schedules_paths_and_urls() {
        for (config, message) in [
            (
                "[app]\nname = \"a\"\nurl = \"https://a.dev\"\n[[crons]]\npath = \"api/x\"\nschedule = \"* * * * *\"\n",
                "must start with `/`",
            ),
            (
                "[app]\nname = \"a\"\nurl = \"https://a.dev\"\n[[crons]]\npath = \"/api/x\"\nschedule = \"* * * *\"\n",
                "must have 5 fields",
            ),
            (
                "[app]\nname = \"a\"\nurl = \"a.dev\"\n",
                "absolute http(s) URL",
            ),
        ] {
            let dir = tempdir("invalid");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(CONFIG_FILE), config).unwrap();
            let error = load_config(&dir).unwrap_err();
            assert!(error.contains(message), "{error}");
        }
    }
}
