//! Turso/libsql [`JobStore`] backend (feature `libsql`).
//!
//! Connects lazily on first use (the store is resolved from a sync context,
//! libsql connects async) and auto-migrates once per process: `CREATE TABLE IF
//! NOT EXISTS`, then additive `ADD COLUMN`s for tables created by older
//! versions. Claiming uses a conditional `UPDATE ... WHERE status IN
//! ('queued','failed')` and checks `rows_affected`, so double delivery loses
//! the race atomically at the database. Attempt history is read-modify-write:
//! only the runner that claimed the row writes it.

use super::{
    JobAttempt, JobId, JobQuery, JobRow, JobStatus, StoreError, StoreFuture, now_ms, push_history,
};

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
CREATE INDEX IF NOT EXISTS __nextrs_jobs_due ON __nextrs_jobs (status, next_run_at);
CREATE INDEX IF NOT EXISTS __nextrs_jobs_created ON __nextrs_jobs (created_at);";

/// Remote libsql (Turso) job store.
pub struct LibsqlJobStore {
    url: String,
    token: String,
    conn: tokio::sync::OnceCell<libsql::Connection>,
}

/// Build from env: `NEXTRS_JOBS_DB_URL` → `NEXTRS_DB_URL` →
/// `TURSO_DATABASE_URL` (tokens likewise); `None` when none is set.
pub(super) fn from_env() -> Option<LibsqlJobStore> {
    let (url, token) = crate::db::env_url(&["NEXTRS_JOBS_DB_URL"], &["NEXTRS_JOBS_DB_TOKEN"])?;
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
                let conn = crate::db::connect(&self.url, &self.token, MIGRATION)
                    .await
                    .map_err(|e| StoreError(format!("jobs {e}")))?;
                // v2 columns, added to tables created by v1.
                for col in ["result TEXT", "history TEXT NOT NULL DEFAULT '[]'"] {
                    crate::db::add_column(&conn, "__nextrs_jobs", col)
                        .await
                        .map_err(StoreError)?;
                }
                Ok(conn)
            })
            .await
    }

    async fn fetch_one(&self, id: &str) -> Result<Option<JobRow>, StoreError> {
        let mut rows = self
            .conn()
            .await?
            .query(
                &format!("SELECT {COLS} FROM __nextrs_jobs WHERE id = ?1"),
                libsql::params![id.to_string()],
            )
            .await
            .map_err(|e| StoreError(format!("get: {e}")))?;
        match rows.next().await.map_err(err)? {
            Some(row) => Ok(Some(row_from_sql(&row)?)),
            None => Ok(None),
        }
    }

    /// The row's history with `attempt` appended (capped), as JSON.
    async fn history_with(&self, id: &JobId, attempt: JobAttempt) -> Result<String, StoreError> {
        let mut history = self
            .fetch_one(&id.0)
            .await?
            .map(|r| r.history)
            .unwrap_or_default();
        push_history(&mut history, attempt);
        serde_json::to_string(&history).map_err(err)
    }
}

fn err(e: impl std::fmt::Display) -> StoreError {
    StoreError(e.to_string())
}

const COLS: &str = "id, name, payload, status, attempts, max_attempts, next_run_at, last_error, created_at, updated_at, result, history";

fn row_from_sql(row: &libsql::Row) -> Result<JobRow, StoreError> {
    let get_str = |i| -> Result<String, StoreError> { row.get::<String>(i).map_err(err) };
    let status_raw = get_str(3)?;
    Ok(JobRow {
        id: JobId(get_str(0)?),
        name: get_str(1)?,
        payload: serde_json::from_str(&get_str(2)?).unwrap_or(serde_json::Value::Null),
        status: JobStatus::parse(&status_raw)
            .ok_or_else(|| StoreError(format!("unknown job status `{status_raw}`")))?,
        attempts: row.get::<u32>(4).map_err(err)?,
        max_attempts: row.get::<u32>(5).map_err(err)?,
        next_run_at: row.get::<Option<i64>>(6).map_err(err)?,
        last_error: row.get::<Option<String>>(7).map_err(err)?,
        created_at: row.get::<i64>(8).map_err(err)?,
        updated_at: row.get::<i64>(9).map_err(err)?,
        result: row
            .get::<Option<String>>(10)
            .map_err(err)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        history: row
            .get::<Option<String>>(11)
            .map_err(err)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
    })
}

impl super::JobStore for LibsqlJobStore {
    fn insert(&self, row: JobRow) -> StoreFuture<'_, ()> {
        Box::pin(async move {
            let payload = serde_json::to_string(&row.payload).map_err(err)?;
            let history = serde_json::to_string(&row.history).map_err(err)?;
            let result = row
                .result
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(err)?;
            self.conn()
                .await?
                .execute(
                    &format!(
                        "INSERT INTO __nextrs_jobs ({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"
                    ),
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
                        row.updated_at,
                        result,
                        history
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
            let affected = self
                .conn()
                .await?
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
            self.fetch_one(&id.0).await
        })
    }

    fn mark_succeeded(&self, id: &JobId, attempt: JobAttempt, result: serde_json::Value) -> StoreFuture<'_, ()> {
        let id = id.clone();
        Box::pin(async move {
            let attempt_n = attempt.n;
            let history = self.history_with(&id, attempt).await?;
            let result = serde_json::to_string(&result).map_err(err)?;
            self.conn()
                .await?
                .execute(
                    "UPDATE __nextrs_jobs
                     SET status = 'succeeded', next_run_at = NULL, result = ?2,
                         history = ?3, updated_at = ?4
                     WHERE id = ?1 AND status = 'running' AND attempts = ?5",
                    libsql::params![id.0, result, history, now_ms(), attempt_n],
                )
                .await
                .map_err(|e| StoreError(format!("mark_succeeded: {e}")))?;
            Ok(())
        })
    }

    fn mark_failed(&self, id: &JobId, attempt: JobAttempt, next_run_at: Option<i64>) -> StoreFuture<'_, ()> {
        let id = id.clone();
        Box::pin(async move {
            let status = if next_run_at.is_some() { "failed" } else { "dead" };
            let error = attempt.error.clone();
            let attempt_n = attempt.n;
            let history = self.history_with(&id, attempt).await?;
            self.conn()
                .await?
                .execute(
                    "UPDATE __nextrs_jobs
                     SET status = ?2, next_run_at = ?3, last_error = ?4, history = ?5, updated_at = ?6
                     WHERE id = ?1 AND status = 'running' AND attempts = ?7",
                    libsql::params![id.0, status, next_run_at, error, history, now_ms(), attempt_n],
                )
                .await
                .map_err(|e| StoreError(format!("mark_failed: {e}")))?;
            Ok(())
        })
    }

    fn requeue(&self, id: &JobId) -> StoreFuture<'_, Option<JobRow>> {
        let id = id.clone();
        Box::pin(async move {
            let now = now_ms();
            let affected = self
                .conn()
                .await?
                .execute(
                    "UPDATE __nextrs_jobs
                     SET status = 'queued', next_run_at = ?2, updated_at = ?2
                     WHERE id = ?1 AND status IN ('failed', 'dead', 'succeeded')",
                    libsql::params![id.0.clone(), now],
                )
                .await
                .map_err(|e| StoreError(format!("requeue: {e}")))?;
            if affected == 0 {
                return Ok(None);
            }
            self.fetch_one(&id.0).await
        })
    }

    fn prune(&self, cutoff: i64) -> StoreFuture<'_, u64> {
        Box::pin(async move {
            self.conn()
                .await?
                .execute(
                    "DELETE FROM __nextrs_jobs
                     WHERE status IN ('succeeded', 'dead') AND updated_at < ?1",
                    libsql::params![cutoff],
                )
                .await
                .map_err(|e| StoreError(format!("prune: {e}")))
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
            while let Some(row) = rows.next().await.map_err(err)? {
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
                     SET status = CASE WHEN attempts >= max_attempts THEN 'dead' ELSE 'failed' END,
                         next_run_at = CASE WHEN attempts >= max_attempts THEN NULL ELSE ?2 END,
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
        Box::pin(async move { self.fetch_one(&id.0).await })
    }

    fn list(&self, query: JobQuery) -> StoreFuture<'_, Vec<JobRow>> {
        Box::pin(async move {
            let mut sql = format!("SELECT {COLS} FROM __nextrs_jobs WHERE 1=1");
            let mut params: Vec<libsql::Value> = Vec::new();
            if let Some(status) = query.status {
                params.push(status.as_str().to_string().into());
                sql.push_str(&format!(" AND status = ?{}", params.len()));
            }
            if let Some(name) = &query.name {
                params.push(name.clone().into());
                sql.push_str(&format!(" AND name = ?{}", params.len()));
            }
            if let Some(before) = query.before {
                params.push(before.into());
                sql.push_str(&format!(" AND created_at < ?{}", params.len()));
            }
            params.push((query.limit() as i64).into());
            sql.push_str(&format!(" ORDER BY created_at DESC LIMIT ?{}", params.len()));
            let mut rows = self
                .conn()
                .await?
                .query(&sql, params)
                .await
                .map_err(|e| StoreError(format!("list: {e}")))?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await.map_err(err)? {
                out.push(row_from_sql(&row)?);
            }
            Ok(out)
        })
    }
}

/// Runs against a real libsql server when `NEXTRS_TEST_LIBSQL_URL` is set
/// (e.g. `sqld --http-listen-addr 127.0.0.1:8089`); skipped otherwise.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::JobStore as _;

    #[tokio::test]
    async fn libsql_job_store_lifecycle() {
        let Ok(url) = std::env::var("NEXTRS_TEST_LIBSQL_URL") else {
            eprintln!("skipped: NEXTRS_TEST_LIBSQL_URL unset");
            return;
        };
        let s = LibsqlJobStore::new(url, "");
        let id = JobId(format!("t{}", now_ms()));
        let attempt = |n, error: Option<&str>| JobAttempt {
            n,
            started_at: now_ms(),
            ms: 2,
            error: error.map(Into::into),
            lines: vec![],
            dropped_lines: 0,
        };
        s.insert(JobRow {
            id: id.clone(),
            name: "libsql-test".into(),
            payload: serde_json::json!({"x": 1}),
            status: JobStatus::Queued,
            attempts: 0,
            max_attempts: 2,
            next_run_at: Some(now_ms()),
            last_error: None,
            created_at: now_ms(),
            updated_at: now_ms(),
            result: None,
            history: vec![],
        })
        .await
        .unwrap();

        let claimed = s.claim(&id).await.unwrap().unwrap();
        assert_eq!((claimed.status, claimed.attempts), (JobStatus::Running, 1));
        assert!(s.claim(&id).await.unwrap().is_none(), "double claim must lose");

        s.mark_failed(&id, attempt(1, Some("boom")), Some(now_ms() - 1)).await.unwrap();
        assert!(s.due(now_ms(), 50).await.unwrap().iter().any(|r| r.id == id));
        s.claim(&id).await.unwrap().unwrap();
        s.mark_succeeded(&id, attempt(2, None), serde_json::json!({"ok": 1})).await.unwrap();

        let row = s.get(&id).await.unwrap().unwrap();
        assert_eq!(row.status, JobStatus::Succeeded);
        assert_eq!(row.result, Some(serde_json::json!({"ok": 1})));
        assert_eq!(row.history.iter().map(|a| a.n).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(row.last_error.as_deref(), Some("boom"));

        let listed = s
            .list(JobQuery { status: Some(JobStatus::Succeeded), name: Some("libsql-test".into()), ..Default::default() })
            .await
            .unwrap();
        assert!(listed.iter().any(|r| r.id == id));

        // A stale report for an older attempt is ignored.
        s.mark_failed(&id, attempt(1, Some("zombie")), None).await.unwrap();
        assert_eq!(s.get(&id).await.unwrap().unwrap().status, JobStatus::Succeeded);

        let requeued = s.requeue(&id).await.unwrap().unwrap();
        assert_eq!(requeued.status, JobStatus::Queued);
        assert!(s.requeue(&id).await.unwrap().is_none());

        // A run that killed its instance with no attempts left is reclaimed
        // as dead, not retried forever.
        let crash = JobId(format!("c{}", now_ms()));
        s.insert(JobRow {
            id: crash.clone(),
            name: "libsql-test".into(),
            payload: serde_json::json!({}),
            status: JobStatus::Queued,
            attempts: 0,
            max_attempts: 1,
            next_run_at: Some(now_ms()),
            last_error: None,
            created_at: now_ms(),
            updated_at: now_ms(),
            result: None,
            history: vec![],
        })
        .await
        .unwrap();
        s.claim(&crash).await.unwrap().unwrap();
        assert!(s.reclaim_stale(now_ms() + 10_000).await.unwrap() >= 1);
        let row = s.get(&crash).await.unwrap().unwrap();
        assert_eq!((row.status, row.next_run_at), (JobStatus::Dead, None));
    }
}
