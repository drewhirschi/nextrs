//! Cold-start telemetry ingestion + publishing.
//!
//! The fleet pinger (`.github/workflows/coldstart-pinger.yml`) POSTs each
//! batch of samples here instead of committing them to git; rows land in the
//! Turso database `nextrs-metrics` (table `coldstarts`). GET serves per-app
//! aggregates — the queryable source for publishing fleet cold-start numbers.
//!
//! Auth: POST requires `x-api-key` matching the `METRICS_API_KEY` env var.
//! GET is public (aggregates only). If the Turso env isn't configured
//! (local dev), both verbs answer 503 rather than failing the build or boot.

use axum::Json;
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use utoipa::ToSchema;

/// One pinger observation. `extra` keeps the schema open for expansion —
/// anything the pinger learns to measure lands there without a migration.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct ColdstartSample {
    /// RFC3339 timestamp of the ping.
    pub ts: String,
    /// Fleet app name (metrics/fleet.json).
    pub app: String,
    /// URL that was hit.
    #[serde(default)]
    pub url: Option<String>,
    /// True when the request completed with status < 500.
    #[serde(default)]
    pub ok: bool,
    /// HTTP status (0 on transport error).
    #[serde(default)]
    pub status: Option<i64>,
    /// Full response time in ms.
    #[serde(default)]
    pub ms: Option<i64>,
    /// "cold" | "warm" | "unknown".
    #[serde(default)]
    pub temp: Option<String>,
    /// Instance uptime reported by /__nx/health.
    #[serde(default)]
    pub uptime_ms: Option<i64>,
    /// Instance boot id reported by /__nx/health.
    #[serde(default)]
    pub boot_id: Option<String>,
    /// Transport error, if any.
    #[serde(default)]
    pub error: Option<String>,
    /// Open extension point (JSON object).
    #[serde(default)]
    pub extra: Option<serde_json::Value>,
    /// What was hit: "page" | "api" (older samples: null).
    #[serde(default)]
    pub target: Option<String>,
    /// "burst" (concurrent spike) | "seq" (sequential, browser-like).
    #[serde(default)]
    pub phase: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct IngestResponse {
    pub inserted: usize,
}

/// Per-app aggregate served by GET.
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct AppStats {
    pub app: String,
    /// "page" | "api" | "" for pre-burst samples.
    pub target: String,
    pub samples: i64,
    pub cold: i64,
    pub warm: i64,
    /// First-request-on-an-idle-instance: flagged cold by the process, but
    /// the instance had been up long before (Vercel pre-provisioning), so no
    /// boot cost was paid. Counted separately; latency folded into warm.
    pub prewarmed: i64,
    pub errors: i64,
    pub cold_p50_ms: Option<i64>,
    pub cold_p90_ms: Option<i64>,
    pub cold_p95_ms: Option<i64>,
    pub warm_p50_ms: Option<i64>,
    pub warm_p90_ms: Option<i64>,
    pub warm_p95_ms: Option<i64>,
    /// Burst-phase totals (the concurrency probe): how many requests the
    /// app's instances absorbed, and how many required a fresh instance.
    pub burst_requests: i64,
    pub burst_colds: i64,
    /// Edge POPs that received the requests in this aggregate window.
    pub edge_regions: Vec<String>,
    /// Function regions parsed from x-vercel-id. Empty means no function hop
    /// was reported (normally a CDN response) or legacy telemetry.
    pub function_regions: Vec<String>,
    /// Region configured as the fair-comparison target in metrics/fleet.json.
    pub expected_regions: Vec<String>,
    pub cdn_hits: i64,
    pub cdn_misses: i64,
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct ColdstartStats {
    pub apps: Vec<AppStats>,
    pub total_samples: i64,
    /// Bumped whenever methodology changes invalidate the old population.
    /// Existing raw rows remain recoverable but are excluded from aggregates.
    pub telemetry_version: i64,
}

// v3: the medium pair became symmetric — hhh-nextjs's landing went
// force-dynamic (it was CDN-served, so its function saw half the burst
// concurrency of hhh-rs's, inflating the Rust side's cold-start frequency
// and contention-loaded cold latencies). v2 rows stay in the raw table but
// are excluded from aggregates, same policy as the v1->v2 reset.
const TELEMETRY_VERSION: i64 = 3;

static DB: OnceCell<Option<libsql::Database>> = OnceCell::const_new();

/// GET aggregates cache: recomputing from Turso costs a cross-process query
/// per hit and scales with row count; the landing polls every 60s, so a 30s
/// TTL keeps the data effectively live while making repeat hits ~0ms.
static STATS_CACHE: std::sync::Mutex<Option<(std::time::Instant, ColdstartStats)>> =
    std::sync::Mutex::new(None);
const STATS_TTL: std::time::Duration = std::time::Duration::from_secs(30);

async fn db() -> Option<&'static libsql::Database> {
    DB.get_or_init(|| async {
        let url = std::env::var("TURSO_METRICS_URL").ok()?;
        let token = std::env::var("TURSO_METRICS_AUTH_TOKEN").ok()?;
        libsql::Builder::new_remote(url, token).build().await.ok()
    })
    .await
    .as_ref()
}

fn authorized(headers: &HeaderMap) -> bool {
    let Ok(expected) = std::env::var("METRICS_API_KEY") else {
        return false;
    };
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        // Not secret-compare hardened; the key gates spam, not data reads.
        .is_some_and(|got| !expected.is_empty() && got == expected)
}

#[nextrs::api(
    post,
    operation_id = "ingestColdstarts",
    responses(
        (status = 200, description = "Samples stored", body = IngestResponse),
        (status = 401, description = "Missing or wrong x-api-key"),
        (status = 503, description = "Metrics store not configured"),
    ),
)]
pub async fn post(
    headers: HeaderMap,
    Json(samples): Json<Vec<ColdstartSample>>,
) -> Result<Json<IngestResponse>, StatusCode> {
    if !authorized(&headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let Some(database) = db().await else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let conn = database
        .connect()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let mut inserted = 0usize;
    for s in &samples {
        let extra = s.extra.as_ref().map(|v| v.to_string());
        let res = conn
            .execute(
                "INSERT INTO coldstarts (ts, app, url, ok, status, ms, temp, uptime_ms, boot_id, error, extra, target, phase)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                libsql::params![
                    s.ts.clone(),
                    s.app.clone(),
                    s.url.clone(),
                    s.ok as i64,
                    s.status,
                    s.ms,
                    s.temp.clone(),
                    s.uptime_ms,
                    s.boot_id.clone(),
                    s.error.clone(),
                    extra,
                    s.target.clone(),
                    s.phase.clone(),
                ],
            )
            .await;
        if res.is_ok() {
            inserted += 1;
        }
    }
    if inserted > 0 {
        // Best-effort: a failed snapshot refresh must not fail the ingest;
        // GET falls back to live computation when the snapshot is stale-less.
        let _ = write_snapshot(&conn).await;
    }
    Ok(Json(IngestResponse { inserted }))
}

fn pct(sorted: &[i64], p: f64) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    // Linear interpolation on the zero-based (n - 1) rank, matching the
    // checked-in benchmark scripts. The old floor(n * p) rule made p50 the
    // upper middle value and made p90 the maximum for ten-sample windows.
    let rank = (sorted.len() - 1) as f64 * p;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    let value = if lower == upper {
        sorted[lower] as f64
    } else {
        sorted[lower] as f64 + (sorted[upper] - sorted[lower]) as f64 * (rank - lower as f64)
    };
    Some(value.round() as i64)
}

/// A "cold" flag is only a real cold start if the instance was actually
/// young when it served its first request. Vercel pre-provisions instances
/// ahead of traffic; their first requests report cold but paid no boot.
const REAL_COLD_MAX_UPTIME_MS: i64 = 10_000;

async fn compute_stats(conn: &libsql::Connection) -> Result<ColdstartStats, libsql::Error> {
    let mut rows = conn
        .query(
            "SELECT app, temp, ms, ok, error IS NOT NULL, ts, COALESCE(target, ''), uptime_ms, COALESCE(phase, ''), extra
             FROM coldstarts",
            (),
        )
        .await?;

    use std::collections::{BTreeMap, BTreeSet};
    #[derive(Default)]
    struct Acc {
        cold: Vec<i64>,
        warm: Vec<i64>,
        seq_warm: Vec<i64>,
        prewarmed: i64,
        burst_reqs: i64,
        burst_colds: i64,
        edge_regions: BTreeSet<String>,
        function_regions: BTreeSet<String>,
        expected_regions: BTreeSet<String>,
        cdn_hits: i64,
        cdn_misses: i64,
        errors: i64,
        samples: i64,
        first: Option<String>,
        last: Option<String>,
    }
    let mut by_app: BTreeMap<(String, String), Acc> = BTreeMap::new();
    let mut total = 0i64;
    while let Ok(Some(row)) = rows.next().await {
        let app: String = row.get(0).unwrap_or_default();
        let temp: Option<String> = row.get(1).ok();
        let ms: Option<i64> = row.get(2).ok();
        let ok: i64 = row.get(3).unwrap_or(0);
        let errored: i64 = row.get(4).unwrap_or(0);
        let ts: String = row.get(5).unwrap_or_default();
        let target: String = row.get(6).unwrap_or_default();
        let uptime: Option<i64> = row.get(7).ok();
        let phase: String = row.get(8).unwrap_or_default();
        let extra: Option<String> = row.get(9).ok();
        let extra = extra
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
        // Version 2 is a deliberate clean slate after the original live table
        // mixed regions, delivery modes, sample windows, and sequential probe
        // times. Preserve the raw history, but never blend it into new claims.
        if extra
            .as_ref()
            .and_then(|value| value.get("telemetry_version"))
            .and_then(|value| value.as_i64())
            != Some(TELEMETRY_VERSION)
        {
            continue;
        }
        // For comparison pairs, a wrong region invalidates the whole pair's
        // target in that batch (the pinger stamps both sides false). This
        // keeps their sample windows aligned while a deployment is being
        // moved; the rejected rows remain available in the raw table.
        if extra
            .as_ref()
            .and_then(|value| value.get("pair_region_match"))
            .and_then(|value| value.as_bool())
            == Some(false)
        {
            continue;
        }
        let acc = by_app.entry((app, target)).or_default();
        if let Some(extra) = extra {
            let remember = |field: &str, values: &mut BTreeSet<String>| {
                if let Some(value) = extra.get(field).and_then(|v| v.as_str()) {
                    if !value.is_empty() {
                        values.insert(value.to_owned());
                    }
                }
            };
            remember("edge_region", &mut acc.edge_regions);
            remember("function_region", &mut acc.function_regions);
            remember("expected_region", &mut acc.expected_regions);
            match extra.get("vercel_cache").and_then(|v| v.as_str()) {
                Some("HIT") => acc.cdn_hits += 1,
                Some("MISS") => acc.cdn_misses += 1,
                _ => {}
            }
        }
        acc.samples += 1;
        total += 1;
        if phase == "burst" && ok == 1 && errored == 0 {
            acc.burst_reqs += 1;
            if temp.as_deref() == Some("cold")
                && uptime.is_some_and(|value| value <= REAL_COLD_MAX_UPTIME_MS)
            {
                acc.burst_colds += 1;
            }
        }
        if ok == 0 || errored == 1 {
            acc.errors += 1;
        } else if let Some(ms) = ms {
            match temp.as_deref() {
                Some("cold") if uptime.is_some_and(|value| value <= REAL_COLD_MAX_UPTIME_MS) => {
                    acc.cold.push(ms)
                }
                Some("cold") => {
                    acc.prewarmed += 1;
                    acc.warm.push(ms);
                }
                Some("warm") => {
                    // Only explicitly-sequential samples are browser-like.
                    // (NOT phase != "burst": rows ingested before the phase
                    // column exist with NULL phase, and most of those were
                    // bursts — that inference already burned us once.)
                    if phase == "seq" {
                        acc.seq_warm.push(ms);
                    }
                    acc.warm.push(ms);
                }
                // Next.js pages can't stamp instance-temperature headers, so
                // their samples come back "unknown". Sequential-phase hits
                // land on burst-warmed instances, so their timings ARE the
                // browser-like "typical" numbers — count them as warm.
                Some("unknown") | None if phase == "seq" => {
                    acc.seq_warm.push(ms);
                    acc.warm.push(ms);
                }
                _ => {}
            }
        }
        if acc.first.as_deref().is_none_or(|f| ts.as_str() < f) {
            acc.first = Some(ts.clone());
        }
        if acc.last.as_deref().is_none_or(|l| ts.as_str() > l) {
            acc.last = Some(ts);
        }
    }

    let apps = by_app
        .into_iter()
        .map(|((app, target), mut a)| {
            a.cold.sort_unstable();
            a.warm.sort_unstable();
            a.seq_warm.sort_unstable();
            // Browser-like warm numbers when we have them; spike warms as
            // fallback for apps that predate the sequential phase.
            let warm_src = if a.seq_warm.is_empty() {
                &a.warm
            } else {
                &a.seq_warm
            };
            AppStats {
                app,
                target,
                samples: a.samples,
                cold: a.cold.len() as i64,
                warm: a.warm.len() as i64 - a.prewarmed,
                prewarmed: a.prewarmed,
                errors: a.errors,
                cold_p50_ms: pct(&a.cold, 0.50),
                cold_p90_ms: pct(&a.cold, 0.90),
                cold_p95_ms: pct(&a.cold, 0.95),
                warm_p50_ms: pct(warm_src, 0.50),
                warm_p90_ms: pct(warm_src, 0.90),
                warm_p95_ms: pct(warm_src, 0.95),
                burst_requests: a.burst_reqs,
                burst_colds: a.burst_colds,
                edge_regions: a.edge_regions.into_iter().collect(),
                function_regions: a.function_regions.into_iter().collect(),
                expected_regions: a.expected_regions.into_iter().collect(),
                cdn_hits: a.cdn_hits,
                cdn_misses: a.cdn_misses,
                first_ts: a.first,
                last_ts: a.last,
            }
        })
        .collect();

    Ok(ColdstartStats {
        apps,
        total_samples: total,
        telemetry_version: TELEMETRY_VERSION,
    })
}

#[cfg(test)]
mod tests {
    use super::pct;

    #[test]
    fn percentile_uses_interpolated_rank() {
        assert_eq!(pct(&[], 0.5), None);
        assert_eq!(pct(&[10], 0.9), Some(10));
        assert_eq!(pct(&[10, 20], 0.5), Some(15));
        assert_eq!(pct(&[0, 10, 20, 30], 0.9), Some(27));
    }
}

/// Recompute aggregates and persist them as a single snapshot row, so GET is
/// one tiny fetch instead of an all-rows scan. Called from POST — the stats
/// only change when new samples arrive.
async fn write_snapshot(conn: &libsql::Connection) -> Result<(), libsql::Error> {
    let stats = compute_stats(conn).await?;
    let json = serde_json::to_string(&stats).unwrap_or_default();
    conn.execute(
        "INSERT INTO stats_snapshot (id, json, updated_at) VALUES (1, ?1, datetime('now'))
         ON CONFLICT(id) DO UPDATE SET json = ?1, updated_at = datetime('now')",
        libsql::params![json],
    )
    .await?;
    Ok(())
}

#[nextrs::api(
    get,
    operation_id = "getColdstartStats",
    responses(
        (status = 200, description = "Per-app cold/warm aggregates (empty when the store is unconfigured, e.g. local dev)", body = ColdstartStats),
    ),
)]
pub async fn get() -> Result<Json<ColdstartStats>, StatusCode> {
    if let Some((at, cached)) = STATS_CACHE.lock().unwrap().as_ref() {
        if at.elapsed() < STATS_TTL {
            return Ok(Json(cached.clone()));
        }
    }
    // Unconfigured (local dev / CI): empty aggregates, not an error — the
    // landing page calls this on every load and a 503 would log console
    // noise in every environment without the Turso env.
    let Some(database) = db().await else {
        return Ok(Json(ColdstartStats {
            apps: vec![],
            total_samples: 0,
            telemetry_version: TELEMETRY_VERSION,
        }));
    };
    let conn = database
        .connect()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    // Snapshot first: one small row, precomputed at ingest. Fall back to a
    // live computation only when no snapshot exists yet.
    let snapshot: Option<String> = match conn
        .query("SELECT json FROM stats_snapshot WHERE id = 1", ())
        .await
    {
        Ok(mut rows) => rows
            .next()
            .await
            .ok()
            .flatten()
            .and_then(|row| row.get::<String>(0).ok()),
        Err(_) => None,
    };
    let stats = match snapshot.and_then(|j| serde_json::from_str::<ColdstartStats>(&j).ok()) {
        Some(stats) => stats,
        None => compute_stats(&conn)
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
    };

    *STATS_CACHE.lock().unwrap() = Some((std::time::Instant::now(), stats.clone()));
    Ok(Json(stats))
}
