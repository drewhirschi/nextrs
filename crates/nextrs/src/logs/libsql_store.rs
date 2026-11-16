//! Turso/libsql [`LogStore`] backend (feature `libsql`): one row per kept
//! request in `__nextrs_logs`, filter columns broken out, the full record as
//! JSON. Connects lazily and auto-migrates once per process.

use super::{LogQuery, LogStore, LogStoreError, RequestLog, StoreFuture, level_rank};

const MIGRATION: &str = "\
CREATE TABLE IF NOT EXISTS __nextrs_logs (
  id      TEXT PRIMARY KEY,
  ts      INTEGER NOT NULL,
  method  TEXT NOT NULL,
  route   TEXT NOT NULL,
  status  INTEGER NOT NULL,
  ms      REAL NOT NULL,
  level   INTEGER,
  record  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS __nextrs_logs_ts ON __nextrs_logs (ts);
CREATE INDEX IF NOT EXISTS __nextrs_logs_route_ts ON __nextrs_logs (route, ts);";

/// Remote libsql (Turso) log store.
pub struct LibsqlLogStore {
    url: String,
    token: String,
    conn: tokio::sync::OnceCell<libsql::Connection>,
}

impl LibsqlLogStore {
    pub fn new(url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            token: token.into(),
            conn: tokio::sync::OnceCell::new(),
        }
    }

    async fn conn(&self) -> Result<&libsql::Connection, LogStoreError> {
        self.conn
            .get_or_try_init(|| async {
                crate::db::connect(&self.url, &self.token, MIGRATION)
                    .await
                    .map_err(LogStoreError)
            })
            .await
    }
}

fn err(e: impl std::fmt::Display) -> LogStoreError {
    LogStoreError(e.to_string())
}

fn decode(row: &libsql::Row) -> Result<RequestLog, LogStoreError> {
    let json: String = row.get(0).map_err(err)?;
    serde_json::from_str(&json).map_err(err)
}

impl LogStore for LibsqlLogStore {
    fn insert(&self, record: RequestLog) -> StoreFuture<'_, ()> {
        Box::pin(async move {
            let json = serde_json::to_string(&record).map_err(err)?;
            let level = record.level.as_deref().map(|l| level_rank(l) as i64);
            self.conn()
                .await?
                .execute(
                    "INSERT OR REPLACE INTO __nextrs_logs (id, ts, method, route, status, ms, level, record)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    libsql::params![
                        record.id,
                        record.ts,
                        record.method,
                        record.route,
                        record.status as i64,
                        record.ms,
                        level,
                        json
                    ],
                )
                .await
                .map_err(|e| LogStoreError(format!("insert: {e}")))?;
            Ok(())
        })
    }

    fn query(&self, query: LogQuery) -> StoreFuture<'_, Vec<RequestLog>> {
        Box::pin(async move {
            let mut sql = String::from("SELECT record FROM __nextrs_logs WHERE 1=1");
            let mut params: Vec<libsql::Value> = Vec::new();
            if let Some(route) = &query.route {
                params.push(route.clone().into());
                sql.push_str(&format!(" AND route = ?{}", params.len()));
            }
            if let Some(min) = query.status_min {
                params.push((min as i64).into());
                sql.push_str(&format!(" AND status >= ?{}", params.len()));
            }
            if let Some(level) = &query.level {
                params.push((level_rank(level) as i64).into());
                sql.push_str(&format!(" AND level >= ?{}", params.len()));
            }
            if let Some(since) = query.since {
                params.push(since.into());
                sql.push_str(&format!(" AND ts >= ?{}", params.len()));
            }
            if let Some(before) = query.before {
                params.push(before.into());
                sql.push_str(&format!(" AND ts < ?{}", params.len()));
            }
            params.push((query.limit() as i64).into());
            sql.push_str(&format!(" ORDER BY ts DESC LIMIT ?{}", params.len()));
            let mut rows = self
                .conn()
                .await?
                .query(&sql, params)
                .await
                .map_err(|e| LogStoreError(format!("query: {e}")))?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await.map_err(err)? {
                out.push(decode(&row)?);
            }
            Ok(out)
        })
    }

    fn get(&self, id: &str) -> StoreFuture<'_, Option<RequestLog>> {
        let id = id.to_string();
        Box::pin(async move {
            let mut rows = self
                .conn()
                .await?
                .query("SELECT record FROM __nextrs_logs WHERE id = ?1", libsql::params![id])
                .await
                .map_err(|e| LogStoreError(format!("get: {e}")))?;
            match rows.next().await.map_err(err)? {
                Some(row) => Ok(Some(decode(&row)?)),
                None => Ok(None),
            }
        })
    }

    fn prune(&self, cutoff: i64) -> StoreFuture<'_, u64> {
        Box::pin(async move {
            self.conn()
                .await?
                .execute("DELETE FROM __nextrs_logs WHERE ts < ?1", libsql::params![cutoff])
                .await
                .map_err(|e| LogStoreError(format!("prune: {e}")))
        })
    }
}

/// Runs against a real libsql server when `NEXTRS_TEST_LIBSQL_URL` is set;
/// skipped otherwise.
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn libsql_log_store_round_trip() {
        let Ok(url) = std::env::var("NEXTRS_TEST_LIBSQL_URL") else {
            eprintln!("skipped: NEXTRS_TEST_LIBSQL_URL unset");
            return;
        };
        let s = LibsqlLogStore::new(url, "");
        let route = format!("/api/libsql-test-{}", crate::logs::now_ms());
        let rec = |id: &str, ts: i64, status: u16, level: Option<&str>| RequestLog {
            id: id.into(),
            ts,
            method: "POST".into(),
            route: route.clone(),
            status,
            ms: 12.5,
            cold: false,
            level: level.map(Into::into),
            segments: vec![("handler".into(), 11.0)],
            lines: vec![crate::logs::LogLine {
                t_ms: 1.0,
                level: "info".into(),
                target: "t".into(),
                msg: "hello".into(),
                fields: Default::default(),
                after_response: false,
            }],
            dropped_lines: 0,
        };
        let base = crate::logs::now_ms();
        s.insert(rec(&format!("{route}-a"), base, 200, Some("info"))).await.unwrap();
        s.insert(rec(&format!("{route}-b"), base + 1, 500, Some("error"))).await.unwrap();

        let all = s.query(LogQuery { route: Some(route.clone()), ..Default::default() }).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].status, 500, "newest first");
        assert_eq!(all[1].lines[0].msg, "hello");

        let errors = s
            .query(LogQuery { route: Some(route.clone()), level: Some("warn".into()), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(errors.len(), 1);
        let five = s
            .query(LogQuery { route: Some(route.clone()), status_min: Some(500), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(five.len(), 1);
        assert!(s.get(&format!("{route}-a")).await.unwrap().is_some());
        assert!(s.prune(base + 1).await.unwrap() >= 1);
        assert!(s.get(&format!("{route}-a")).await.unwrap().is_none());
    }
}
