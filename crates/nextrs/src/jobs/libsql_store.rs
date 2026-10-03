//! Turso/libsql [`JobStore`] backend (feature `jobs-libsql`).
//!
//! Connects lazily on first use (the store is resolved from a sync context,
//! libsql connects async) and auto-migrates: `CREATE TABLE IF NOT EXISTS`
//! once per process. Claiming uses a conditional `UPDATE ... WHERE status IN
//! ('queued','failed')` and checks `rows_affected`, so double delivery loses
//! the race atomically at the database.

use super::{JobId, JobRow, JobStatus, StoreError, StoreFuture, now_ms};

const MIGRATION: &str = "\
CREATE TABLE IF NOT EXISTS __nextrs_jobs (
  id           TEXT PRIMARY KEY,
  name         TEXT NOT NULL,
  payload      TEXT NOT NULL,
  status       TEXT NOT NULL,
  attempts     INTEGER NOT NULL DEFAULT 0,
  max_attempts INTEGER NOT NULL,
  next_run_at  INTEGER,
  last_error   TEXT,
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS __nextrs_jobs_due ON __nextrs_jobs (status, next_run_at);";

/// Remote libsql (Turso) job store.
pub struct LibsqlJobStore {
    url: String,
    token: String,
    conn: tokio::sync::OnceCell<libsql::Connection>,
}

/// Build from the conventional env vars; `None` when they're absent.
pub(super) fn from_env() -> Option<LibsqlJobStore> {
    let url = std::env::var("NEXTRS_JOBS_DB_URL")
        .or_else(|_| std::env::var("TURSO_DATABASE_URL"))
        .ok()?;
    let token = std::env::var("NEXTRS_JOBS_DB_TOKEN")
        .or_else(|_| std::env::var("TURSO_AUTH_TOKEN"))
        .unwrap_or_default();
    Some(LibsqlJobStore::new(url, token))
}

impl LibsqlJobStore {
    pub fn new(url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            token: token.into(),
            conn: tokio::sync::OnceCell::new(),
        }
    }

    async fn conn(&self) -> Result<&libsql::Connection, StoreError> {
        self.conn
            .get_or_try_init(|| async {
                let db = libsql::Builder::new_remote(self.url.clone(), self.token.clone())
                    .build()
                    .await
                    .map_err(|e| StoreError(format!("libsql connect: {e}")))?;
                let conn = db
                    .connect()
                    .map_err(|e| StoreError(format!("libsql connect: {e}")))?;
                conn.execute_batch(MIGRATION)
                    .await
                    .map_err(|e| StoreError(format!("jobs migration: {e}")))?;
                Ok(conn)
            })
            .await
    }
}

fn row_from_sql(row: &libsql::Row) -> Result<JobRow, StoreError> {
    let get_str = |i| -> Result<String, StoreError> {
        row.get::<String>(i).map_err(|e| StoreError(e.to_string()))
    };
    let status_raw = get_str(3)?;
    Ok(JobRow {
        id: JobId(get_str(0)?),
        name: get_str(1)?,
        payload: serde_json::from_str(&get_str(2)?).unwrap_or(serde_json::Value::Null),
        status: JobStatus::parse(&status_raw)
            .ok_or_else(|| StoreError(format!("unknown job status `{status_raw}`")))?,
        attempts: row.get::<u32>(4).map_err(|e| StoreError(e.to_string()))?,
        max_attempts: row.get::<u32>(5).map_err(|e| StoreError(e.to_string()))?,
        next_run_at: row
            .get::<Option<i64>>(6)
            .map_err(|e| StoreError(e.to_string()))?,
        last_error: row
            .get::<Option<String>>(7)
            .map_err(|e| StoreError(e.to_string()))?,
        created_at: row.get::<i64>(8).map_err(|e| StoreError(e.to_string()))?,
        updated_at: row.get::<i64>(9).map_err(|e| StoreError(e.to_string()))?,
    })
}

const COLS: &str = "id, name, payload, status, attempts, max_attempts, next_run_at, last_error, created_at, updated_at";

impl super::JobStore for LibsqlJobStore {
    fn insert(&self, row: JobRow) -> StoreFuture<'_, ()> {
        Box::pin(async move {
            let payload =
                serde_json::to_string(&row.payload).map_err(|e| StoreError(e.to_string()))?;
            self.conn()
                .await?
                .execute(
                    &format!("INSERT INTO __nextrs_jobs ({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"),
                    libsql::params![
                        row.id.0,
                        row.name,
                        payload,
                        row.status.as_str(),
                        row.attempts,
                        row.max_attempts,
                        row.next_run_at,
                        row.last_error,
                        row.created_at,
                        row.updated_at
                    ],
                )
                .await
                .map_err(|e| StoreError(format!("insert: {e}")))?;
            Ok(())
        })
    }

    fn claim(&self, id: &JobId) -> StoreFuture<'_, Option<JobRow>> {
        let id = id.clone();
        Box::pin(async move {
            let conn = self.conn().await?;
            let affected = conn
                .execute(
                    "UPDATE __nextrs_jobs
                     SET status = 'running', attempts = attempts + 1,
                         next_run_at = NULL, updated_at = ?2
                     WHERE id = ?1 AND status IN ('queued', 'failed')",
                    libsql::params![id.0.clone(), now_ms()],
                )
                .await
                .map_err(|e| StoreError(format!("claim: {e}")))?;
            if affected == 0 {
                return Ok(None);
            }
            let mut rows = conn
                .query(
                    &format!("SELECT {COLS} FROM __nextrs_jobs WHERE id = ?1"),
                    libsql::params![id.0],
                )
                .await
                .map_err(|e| StoreError(format!("claim select: {e}")))?;
            match rows.next().await.map_err(|e| StoreError(e.to_string()))? {
                Some(row) => Ok(Some(row_from_sql(&row)?)),
                None => Ok(None),
            }
        })
    }

    fn mark_succeeded(&self, id: &JobId) -> StoreFuture<'_, ()> {
        let id = id.clone();
        Box::pin(async move {
            self.conn()
                .await?
                .execute(
                    "UPDATE __nextrs_jobs
                     SET status = 'succeeded', next_run_at = NULL, updated_at = ?2
                     WHERE id = ?1",
                    libsql::params![id.0, now_ms()],
                )
                .await
                .map_err(|e| StoreError(format!("mark_succeeded: {e}")))?;
            Ok(())
        })
    }

    fn mark_failed(&self, id: &JobId, error: &str, next_run_at: Option<i64>) -> StoreFuture<'_, ()> {
        let id = id.clone();
        let error = error.to_string();
        Box::pin(async move {
            let status = if next_run_at.is_some() { "failed" } else { "dead" };
            self.conn()
                .await?
                .execute(
                    "UPDATE __nextrs_jobs
                     SET status = ?2, next_run_at = ?3, last_error = ?4, updated_at = ?5
                     WHERE id = ?1",
                    libsql::params![id.0, status, next_run_at, error, now_ms()],
                )
                .await
                .map_err(|e| StoreError(format!("mark_failed: {e}")))?;
            Ok(())
        })
    }

    fn due(&self, now: i64, limit: u32) -> StoreFuture<'_, Vec<JobRow>> {
        Box::pin(async move {
            let mut rows = self
                .conn()
                .await?
                .query(
                    &format!(
                        "SELECT {COLS} FROM __nextrs_jobs
                         WHERE status IN ('queued', 'failed') AND next_run_at <= ?1
                         ORDER BY next_run_at ASC LIMIT ?2"
                    ),
                    libsql::params![now, limit],
                )
                .await
                .map_err(|e| StoreError(format!("due: {e}")))?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await.map_err(|e| StoreError(e.to_string()))? {
                out.push(row_from_sql(&row)?);
            }
            Ok(out)
        })
    }

    fn reclaim_stale(&self, cutoff: i64) -> StoreFuture<'_, u32> {
        Box::pin(async move {
            let affected = self
                .conn()
                .await?
                .execute(
                    "UPDATE __nextrs_jobs
                     SET status = 'failed', next_run_at = ?2,
                         last_error = COALESCE(last_error, 'reclaimed: instance died mid-run'),
                         updated_at = ?2
                     WHERE status = 'running' AND updated_at < ?1",
                    libsql::params![cutoff, now_ms()],
                )
                .await
                .map_err(|e| StoreError(format!("reclaim_stale: {e}")))?;
            Ok(affected as u32)
        })
    }

    fn get(&self, id: &JobId) -> StoreFuture<'_, Option<JobRow>> {
        let id = id.clone();
        Box::pin(async move {
            let mut rows = self
                .conn()
                .await?
                .query(
                    &format!("SELECT {COLS} FROM __nextrs_jobs WHERE id = ?1"),
                    libsql::params![id.0],
                )
                .await
                .map_err(|e| StoreError(format!("get: {e}")))?;
            match rows.next().await.map_err(|e| StoreError(e.to_string()))? {
                Some(row) => Ok(Some(row_from_sql(&row)?)),
                None => Ok(None),
            }
        })
    }

    fn recent(&self, limit: u32) -> StoreFuture<'_, Vec<JobRow>> {
        Box::pin(async move {
            let mut rows = self
                .conn()
                .await?
                .query(
                    &format!(
                        "SELECT {COLS} FROM __nextrs_jobs ORDER BY created_at DESC LIMIT ?1"
                    ),
                    libsql::params![limit],
                )
                .await
                .map_err(|e| StoreError(format!("recent: {e}")))?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await.map_err(|e| StoreError(e.to_string()))? {
                out.push(row_from_sql(&row)?);
            }
            Ok(out)
        })
    }
}
