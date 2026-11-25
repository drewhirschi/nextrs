//! Shared libsql/Turso plumbing for the framework's own tables (jobs, logs).
//!
//! Each framework store resolves its database from a store-specific env var
//! first (`NEXTRS_JOBS_DB_URL`, `NEXTRS_LOGS_DB_URL`), then the shared
//! `NEXTRS_DB_URL`, with the matching `*_TOKEN`. Nothing else is consulted.
//!
//! The framework's database is explicit and separate from the application's:
//! `TURSO_DATABASE_URL` (Turso's conventional name, usually the app's own
//! data) is **not** a fallback, so the `__nextrs_` tables never land in an
//! application database unless `NEXTRS_DB_URL` is pointed at it on purpose.
//! Up to 0.7.0 it was a fallback; an app that relied on it gets a one-time
//! warning naming the variable to set.

/// `(url, token)` from the first set url var in `specific`, then the shared
/// `NEXTRS_DB_URL`; the token is looked up the same way (empty when unset —
/// local `turso dev` servers need none).
pub(crate) fn env_url(specific_url: &[&str], specific_token: &[&str]) -> Option<(String, String)> {
    let env = |name: &str| std::env::var(name).ok();
    let found = resolve(&env, specific_url, specific_token);
    if found.is_none() {
        warn_unused_turso_url();
    }
    found
}

/// [`env_url`] over any variable lookup, so the order can be tested without
/// touching the process environment.
fn resolve(
    env: &dyn Fn(&str) -> Option<String>,
    specific_url: &[&str],
    specific_token: &[&str],
) -> Option<(String, String)> {
    let first = |names: &[&str], shared: &str| {
        names
            .iter()
            .copied()
            .chain([shared])
            .find_map(|name| env(name).filter(|v| !v.is_empty()))
    };
    let url = first(specific_url, "NEXTRS_DB_URL")?;
    Some((
        url,
        first(specific_token, "NEXTRS_DB_TOKEN").unwrap_or_default(),
    ))
}

/// Say once, for apps upgrading from 0.7.0, that `TURSO_DATABASE_URL` no
/// longer selects the framework's database. It is read only to warn.
fn warn_unused_turso_url() {
    static WARNED: std::sync::Once = std::sync::Once::new();
    if std::env::var("TURSO_DATABASE_URL").is_ok_and(|v| !v.is_empty()) {
        WARNED.call_once(|| {
            tracing::warn!(
                "nextrs no longer uses TURSO_DATABASE_URL for its own tables (jobs, logs); \
                 set NEXTRS_DB_URL and NEXTRS_DB_TOKEN to the database they should live in"
            );
        });
    }
}

/// Connect to a remote libsql database and run `migration` once.
pub(crate) async fn connect(url: &str, token: &str, migration: &str) -> Result<libsql::Connection, String> {
    let db = libsql::Builder::new_remote(url.to_string(), token.to_string())
        .build()
        .await
        .map_err(|e| format!("libsql connect: {e}"))?;
    let conn = db.connect().map_err(|e| format!("libsql connect: {e}"))?;
    conn.execute_batch(migration)
        .await
        .map_err(|e| format!("migration: {e}"))?;
    Ok(conn)
}

/// `ALTER TABLE ... ADD COLUMN` that tolerates the column already existing —
/// how the framework's tables grow without a migration framework.
#[cfg_attr(not(feature = "jobs"), allow(dead_code))]
pub(crate) async fn add_column(conn: &libsql::Connection, table: &str, column_def: &str) -> Result<(), String> {
    match conn
        .execute(&format!("ALTER TABLE {table} ADD COLUMN {column_def}"), ())
        .await
    {
        Ok(_) => Ok(()),
        Err(e) if e.to_string().contains("duplicate column") => Ok(()),
        Err(e) => Err(format!("add column {column_def}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn resolve_in(vars: &[(&str, &str)]) -> Option<(String, String)> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        resolve(
            &|name| vars.get(name).cloned(),
            &["NEXTRS_JOBS_DB_URL"],
            &["NEXTRS_JOBS_DB_TOKEN"],
        )
    }

    #[test]
    fn the_framework_database_is_explicit() {
        // The application's Turso database is not the framework's.
        let app = [
            ("TURSO_DATABASE_URL", "libsql://app"),
            ("TURSO_AUTH_TOKEN", "app-token"),
        ];
        assert_eq!(resolve_in(&app), None);

        // NEXTRS_DB_URL is; the app's token is not borrowed for it.
        let mut vars = app.to_vec();
        vars.push(("NEXTRS_DB_URL", "libsql://framework"));
        assert_eq!(
            resolve_in(&vars),
            Some(("libsql://framework".into(), String::new()))
        );
        vars.push(("NEXTRS_DB_TOKEN", "fw-token"));
        assert_eq!(
            resolve_in(&vars),
            Some(("libsql://framework".into(), "fw-token".into()))
        );

        // A store-specific database wins over the shared one.
        vars.push(("NEXTRS_JOBS_DB_URL", "libsql://jobs"));
        vars.push(("NEXTRS_JOBS_DB_TOKEN", "jobs-token"));
        assert_eq!(
            resolve_in(&vars),
            Some(("libsql://jobs".into(), "jobs-token".into()))
        );

        // An empty value is the same as unset.
        assert_eq!(resolve_in(&[("NEXTRS_DB_URL", "")]), None);
    }
}
