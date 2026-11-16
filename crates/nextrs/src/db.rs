//! Shared libsql/Turso plumbing for the framework's own tables (jobs, logs).
//!
//! Each framework store resolves its database from a store-specific env var
//! first (`NEXTRS_JOBS_DB_URL`, `NEXTRS_LOGS_DB_URL`), then the shared
//! `NEXTRS_DB_URL`, then Turso's conventional `TURSO_DATABASE_URL`, with the
//! matching `*_TOKEN` / `TURSO_AUTH_TOKEN`. One Turso database per app is the
//! expected setup; the framework's tables are prefixed `__nextrs_`.

/// `(url, token)` from the first set url var in `specific` + the shared
/// fallbacks; the token is looked up the same way (empty when unset — local
/// `turso dev` servers need none).
pub(crate) fn env_url(specific_url: &[&str], specific_token: &[&str]) -> Option<(String, String)> {
    let first = |names: &[&str]| {
        names
            .iter()
            .find_map(|n| std::env::var(n).ok().filter(|v| !v.is_empty()))
    };
    let mut urls: Vec<&str> = specific_url.to_vec();
    urls.extend(["NEXTRS_DB_URL", "TURSO_DATABASE_URL"]);
    let mut tokens: Vec<&str> = specific_token.to_vec();
    tokens.extend(["NEXTRS_DB_TOKEN", "TURSO_AUTH_TOKEN"]);
    let url = first(&urls)?;
    Some((url, first(&tokens).unwrap_or_default()))
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
