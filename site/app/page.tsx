// The docs-site landing page — a React (page.tsx) route, mounted client-side
// into __nx_root__ by the bundle nextrs generates. This page dogfoods the
// React track; the docs pages under /docs stay server-rendered.
import * as React from "react";
import { NextrsMark, useGetColdstartStats, type AppStats } from "@site/client";

const PREFETCH_RS = `// app/prefetch.rs — runs on the server, streaming
// data into the React Query cache before mount.
pub async fn prefetch(req: Request) -> QuerySeed {
    let todos = db::recent_todos().await;
    QuerySeed::new()
        .add(seed_key(["todos"]), &todos)
}`;

function Stat({ value, label }: { value: string; label: string }) {
  return (
    <div className="stat">
      <b>{value}</b>
      <span>{label}</span>
    </div>
  );
}

function Feature({
  n,
  title,
  children,
}: {
  n: string;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="card">
      <span className="num">{n}</span>
      <h3>{title}</h3>
      <p>{children}</p>
    </div>
  );
}

function fmtMs(v: number | null | undefined) {
  return v == null ? "—" : `${v} ms`;
}

const COMPARISONS: {
  label: string;
  detail: string;
  rust: string;
  next?: string;
}[] = [
  {
    label: "Small app",
    detail: "a todo app",
    rust: "react-todos",
    next: "nextjs-todos",
  },
  {
    label: "Medium app",
    detail: "a booking app",
    rust: "hhh-rs",
    next: "hhh-nextjs",
  },
];

// n = size of the sample pool the percentiles come from. Deltas are
// suppressed below MIN_POOL: right after a telemetry reset a p90 over five
// requests is one straggler, not a trend.
type Metric = { p50: number | null; p90: number | null; n: number };
const MIN_POOL = 20;
const MIN_BURSTS = 100;

function pick(apps: AppStats[], app: string | undefined, target: string) {
  return app ? apps.find((a) => a.app === app && a.target === target) : undefined;
}

// One percentile per line: "p50  1373 ms  56% faster". The green delta only
// renders when a comparison baseline is passed (i.e. the pair is comparable).
function MetricCell({
  m,
  vs,
  missingLabel = "—",
  heat = false,
}: {
  m: Metric | null;
  vs?: Metric | null;
  missingLabel?: string;
  /** Color the value itself by how painful it is (amber >= 1s, red >= 3s). */
  heat?: boolean;
}) {
  if (!m || m.p50 == null)
    return (
      <td style={{ padding: "8px 12px", whiteSpace: "nowrap", opacity: missingLabel === "—" ? 1 : 0.65, fontSize: 13 }}>
        {missingLabel}
      </td>
    );
  const line = (label: string, mine: number | null, base: number | null | undefined) => {
    if (mine == null) return null;
    let diff: React.ReactNode = null;
    if (base != null && base > 0) {
      const ratio = mine / base;
      const pct = Math.round(Math.abs(ratio - 1) * 100);
      if (pct >= 1)
        diff = (
          <span
            title={`${label}: ${mine} ms vs ${base} ms = ${ratio.toFixed(3)}`}
            style={{
              color: ratio < 1 ? "var(--ok, #22c55e)" : "var(--warn, #f59e0b)",
              fontWeight: 600,
              fontSize: 12,
              marginLeft: 8,
            }}
          >
            {pct}% {ratio < 1 ? "faster" : "slower"}
          </span>
        );
    }
    const heatColor = heat
      ? mine >= 3000
        ? "var(--bad, #ef4444)"
        : mine >= 1000
          ? "var(--warn, #f59e0b)"
          : undefined
      : undefined;
    return (
      <div style={{ whiteSpace: "nowrap", lineHeight: 1.7 }}>
        <span style={{ opacity: 0.5, fontSize: 11, display: "inline-block", width: 28 }}>{label}</span>
        <span style={heatColor ? { color: heatColor, fontWeight: 600 } : undefined}>{mine} ms</span>
        {diff}
      </div>
    );
  };
  return (
    <td style={{ padding: "8px 12px", verticalAlign: "top" }}>
      {line("p50", m.p50, vs?.p50)}
      {line("p90", m.p90, vs?.p90)}
    </td>
  );
}

// Fine print folded behind a small toggle — the numbers stay front and
// center, the methodology is one click away.
function InfoNote({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <details style={{ marginTop: 8 }}>
      <summary
        style={{ cursor: "pointer", opacity: 0.5, fontSize: 12, userSelect: "none", width: "fit-content" }}
      >
        ⓘ {label}
      </summary>
      <div style={{ opacity: 0.7, fontSize: 13, marginTop: 6 }}>{children}</div>
    </details>
  );
}

function LiveColdstarts() {
  const { data, isLoading, isError } = useGetColdstartStats({
    query: { refetchInterval: 60_000 },
  });
  const stats = data && data.status === 200 ? data.data : undefined;
  if (isLoading) return <p className="live-note">Loading live numbers…</p>;
  if (isError || !stats) return <p className="live-note">Telemetry temporarily unavailable.</p>;
  if (stats.total_samples === 0)
    return <p className="live-note">Collecting first samples — check back shortly.</p>;

  const metric = (app: string | undefined, target: string, kind: "warm" | "cold"): Metric | null => {
    const a = pick(stats.apps, app, target);
    if (!a) return null;
    if (kind === "cold" && !a.cold) return null;
    return kind === "warm"
      ? {
          p50: (a.warm_p50_ms ?? null) as number | null,
          p90: (a.warm_p90_ms ?? null) as number | null,
          n: a.warm_pool ?? 0,
        }
      : {
          p50: (a.cold_p50_ms ?? null) as number | null,
          p90: (a.cold_p90_ms ?? null) as number | null,
          n: a.cold_pool ?? 0,
        };
  };

  const delivery = (a: AppStats | undefined) => {
    if (!a) return "unknown";
    if (a.cdn_hits > 0 && a.cdn_misses === 0) return "cdn";
    if (a.cdn_hits > 0 && a.cdn_misses > 0) return "mixed";
    return a.function_regions.length > 0 ? "function" : "unknown";
  };

  const comparable = (c: (typeof COMPARISONS)[number], target: "page" | "api") => {
    const rust = pick(stats.apps, c.rust, target);
    const next = pick(stats.apps, c.next, target);
    if (!rust || !next) return false;
    if (rust.samples - rust.errors !== next.samples - next.errors) return false;
    if (delivery(rust) !== "function" || delivery(next) !== "function") return false;
    if (rust.function_regions.length !== 1 || next.function_regions.length !== 1) return false;
    if (rust.function_regions[0] !== next.function_regions[0]) return false;
    const expected = new Set([...rust.expected_regions, ...next.expected_regions]);
    return expected.size === 1 && expected.has(rust.function_regions[0]);
  };

  const comparisonStatus = (c: (typeof COMPARISONS)[number], target: "page" | "api") => {
    const rust = pick(stats.apps, c.rust, target);
    const next = pick(stats.apps, c.next, target);
    if (!rust || !next) return `${target}: collecting routing metadata`;
    const rustDelivery = delivery(rust);
    const nextDelivery = delivery(next);
    if (rustDelivery !== nextDelivery) return `${target}: ${rustDelivery} vs ${nextDelivery} — ratios paused`;
    const rustRegions = rust.function_regions.join(", ") || "no function region";
    const nextRegions = next.function_regions.join(", ") || "no function region";
    if (!comparable(c, target)) return `${target}: ${rustRegions} vs ${nextRegions} — ratios paused`;
    return `${target}: matched in ${rustRegions}`;
  };

  // Page and API timings track each other closely, so the table shows one
  // combined number per temperature: the mean of the targets that are
  // comparable for the pair (same region, both on the function path). Where a
  // framework serves its page from the CDN (no function -> no cold start
  // exists), the pair is compared on the API route alone.
  const combine = (ms: (Metric | null)[]): Metric | null => {
    const xs = ms.filter((m): m is Metric => !!m && m.p50 != null);
    if (!xs.length) return null;
    const avg = (sel: (m: Metric) => number | null) => {
      const vs = xs.map(sel).filter((v): v is number => v != null);
      return vs.length ? Math.round(vs.reduce((a, b) => a + b, 0) / vs.length) : null;
    };
    return { p50: avg((m) => m.p50), p90: avg((m) => m.p90), n: Math.min(...xs.map((m) => m.n)) };
  };
  const comparableTargets = (c: (typeof COMPARISONS)[number]) =>
    (["page", "api"] as const).filter((t) => comparable(c, t));
  const combinedMetric = (
    c: (typeof COMPARISONS)[number],
    app: string | undefined,
    kind: "warm" | "cold",
  ): Metric | null => {
    const ts = comparableTargets(c);
    const use = ts.length ? ts : (["page", "api"] as const);
    return combine(use.map((t) => metric(app, t, kind)));
  };

  return (
    <div>
      <div style={{ overflowX: "auto" }}>
        <table className="live-table" style={{ width: "100%", borderCollapse: "collapse", fontSize: 14 }}>
          <thead>
            <tr>
              {["", "", "cold start", "warm response"].map((h, i) => (
                <th key={i} style={{ textAlign: "left", padding: "8px 12px", opacity: 0.6, fontWeight: 600 }}>
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {COMPARISONS.map((c) => {
              const hasComparison = comparableTargets(c).length > 0;
              // Deltas render only when BOTH sides' percentile pools have
              // matured past MIN_POOL — the values themselves always show.
              const baseline = (kind: "warm" | "cold") => {
                if (!hasComparison) return null;
                const mine = combinedMetric(c, c.rust, kind);
                const base = combinedMetric(c, c.next, kind);
                return mine && base && mine.n >= MIN_POOL && base.n >= MIN_POOL ? base : null;
              };
              const rustRow = (
                <tr key={c.label + "rs"}>
                  <td style={{ padding: "8px 12px", fontWeight: 700 }}>nextrs</td>
                  <MetricCell m={combinedMetric(c, c.rust, "cold")} vs={baseline("cold")} />
                  <MetricCell m={combinedMetric(c, c.rust, "warm")} vs={baseline("warm")} />
                </tr>
              );
              if (!c.next) return rustRow;
              return (
                <React.Fragment key={c.label}>
                  <tr style={{ borderTop: "1px solid var(--line, #333)" }}>
                    <td rowSpan={2} style={{ padding: "8px 12px", verticalAlign: "top" }}>
                      <b>{c.label}</b>
                      <div style={{ opacity: 0.55, fontSize: 12 }}>{c.detail}</div>
                    </td>
                    <td style={{ padding: "8px 12px" }}>Next.js</td>
                    <MetricCell m={combinedMetric(c, c.next, "cold")} heat />
                    <MetricCell m={combinedMetric(c, c.next, "warm")} />
                  </tr>
                  {rustRow}
                </React.Fragment>
              );
            })}
          </tbody>
        </table>
      </div>
      <h2 style={{ marginTop: 32, marginBottom: 10, fontSize: 26 }}>How often do you pay a cold start?</h2>
      {COMPARISONS.filter((c) => c.next).map((c) => {
        const js = pick(stats.apps, c.next, "api");
        const rust = pick(stats.apps, c.rust, "api");
        const rate = (a?: AppStats) =>
          a && a.burst_requests > 0 ? a.burst_colds / a.burst_requests : null;
        const jsRate = rate(js);
        const rustRate = rate(rust);
        const ok =
          comparable(c, "api") &&
          jsRate != null &&
          rustRate != null &&
          jsRate > 0 &&
          (js?.burst_requests ?? 0) >= MIN_BURSTS &&
          (rust?.burst_requests ?? 0) >= MIN_BURSTS;
        let body: React.ReactNode = "collecting comparable data…";
        if (ok) {
          const ratio = rustRate / jsRate;
          const pct = Math.round(Math.abs(1 - ratio) * 100);
          const better = ratio < 1;
          body = (
            <>
              nextrs hits a cold start{" "}
              <span
                style={{
                  color: better ? "var(--ok, #22c55e)" : "var(--warn, #f59e0b)",
                  fontWeight: 600,
                }}
              >
                {pct}% {better ? "less" : "more"} often
              </span>{" "}
              under the same burst load — {Math.round(jsRate * 100)} cold starts per 100
              requests for Next.js vs {Math.round(rustRate * 100)} for nextrs.
            </>
          );
        }
        return (
          <p key={c.label} className="live-note" style={{ fontSize: 17, lineHeight: 1.6, margin: "8px 0" }}>
            <b>{c.label}:</b> {body}
          </p>
        );
      })}
      <InfoNote label="how these numbers are measured">
        <p style={{ margin: "0 0 8px" }}>
          Each app is the same product built twice — once with Next.js, once with
          nextrs — deployed to Vercel and probed identically at randomized times
          every ~2 hours, from the same runner in the same moments. The{" "}
          <b>small app</b> is a todo list: React 19 + TanStack Query over a typed
          JSON API. The <b>medium app</b> is a booking system: sessions/auth,
          Postgres, and an admin backend.
        </p>
        <p style={{ margin: "0 0 8px" }}>
          Probes hit each app&apos;s page <i>and</i> an API route; the timings track
          each other closely, so each cell combines the two. <b>Warm</b> =
          sequential requests against a warm instance (what a user clicking
          around experiences). <b>Cold</b> = a request that started a fresh
          instance during a 20-request concurrent burst, so it includes real
          scale-out provisioning and burst contention. Cold-start frequency is
          fresh instances per successful burst request; it does not infer how
          many requests any one instance served. Where a framework serves its
          page from the CDN (no function runs, so no cold start exists), the
          pair is compared on the API route alone.
        </p>
        <p style={{ margin: 0 }}>
          Green/amber ratios appear only when both sides ran in the same
          function region, that region matches the fleet configuration, and both
          used the same delivery path. Current checks:{" "}
          {COMPARISONS.filter((c) => c.next).map((c, i) => (
            <React.Fragment key={c.label + "status"}>
              {i ? " · " : ""}<b>{c.label}:</b>{" "}
              {comparisonStatus(c, "page")}; {comparisonStatus(c, "api")}
            </React.Fragment>
          ))}{" "}
          — {stats.total_samples.toLocaleString()} methodology-v{stats.telemetry_version}{" "}
          samples since the clean reset, aggregated by <code>/api/coldstarts</code>,
          the endpoint this page is calling right now.
        </p>
      </InfoNote>
    </div>
  );
}

export default function Home() {
  return (
    <>
      <section className="hero">
        <div className="shell hero-grid">
          <div>
            <span className="eyebrow">
              <NextrsMark
                size="1.15em"
                style={{ verticalAlign: "-0.2em", marginRight: "0.55em" }}
              />
              Rust · React · Vercel
            </span>
            <h1>
              Engineered for <span className="em">agents</span>.
            </h1>
            <p className="hero-sub">
              A Next.js-style framework where your React app runs on a Rust
              server. Agents write code faster than a Node runtime can absorb —
              Rust gives you the headroom so the features they ship stay fast and
              don&apos;t rot.
            </p>
            <div className="hero-cta">
              <a className="btn btn-primary" href="/docs/getting-started">
                Get started →
              </a>
              <a className="btn btn-ghost" href="/docs">
                Read the docs
              </a>
            </div>
            {/* TODO: swap in a measured Vercel cold-start number (DESIGN.md
                signature risk #3 — proof over claims) once we have the deploy. */}
            <div className="hero-meta">
              <Stat value="1 fn" label="one Vercel function" />
              <Stat value="Rust" label="no GC, no cold-start tax" />
              <Stat value="React 19" label="+ TanStack Query" />
            </div>
          </div>

          <div className="code" aria-hidden="true">
            <div className="code-head">
              <span className="dot" />
              <span>app/ — file-based routing</span>
            </div>
            <pre>
              <code>
                <span className="c-key">app/</span>
                {`
├─ page.tsx        `}
                <span className="c-dim">→ /            (React, this page)</span>
                {`
├─ layout.tsx      `}
                <span className="c-dim">→ shell + nav (stays mounted)</span>
                {`
├─ `}
                <span className="c-fn">prefetch.rs</span>
                {`     `}
                <span className="c-dim">→ prefetch into the RQ cache</span>
                {`
└─ api/
   └─ route.rs     `}
                <span className="c-dim">→ typed JSON endpoint</span>
              </code>
            </pre>
          </div>
        </div>
      </section>

      <section className="section">
        <div className="shell">
          <div className="section-head">
            <span className="eyebrow">The thesis</span>
            <h2>Agents out-code what Node can absorb.</h2>
            <p>
              The bottleneck shifted. It&apos;s no longer how fast you can write
              features — it&apos;s whether the runtime can carry them once they&apos;re
              written. nextrs puts that weight on Rust.
            </p>
          </div>
          <div className="cards">
            <Feature n="01" title="Headroom, not hot paths">
              A compiled, GC-free server means agent-generated code doesn&apos;t
              quietly accumulate latency. The slow path is still fast.
            </Feature>
            <Feature n="02" title="Next.js conventions">
              <code>app/</code> directory, file-based routes, layouts, loading
              states. The mental model your tools already know — emitted at build
              time by codegen.
            </Feature>
            <Feature n="03" title="React, the way you write it">
              <code>page.tsx</code> components render client-side under a TanStack
              Query provider. <code>prefetch.rs</code> prefetches server data into the
              cache — first paint, no fetch.
            </Feature>
            <Feature n="04" title="One function on Vercel">
              Deploys as a single Rust serverless function plus a catch-all
              rewrite. No Node runtime at the edge, no cold-start tax.
            </Feature>
          </div>
        </div>
      </section>

      <section className="section">
        <div className="shell hero-grid">
          <div className="section-head" style={{ marginBottom: 0 }}>
            <span className="eyebrow">The signature move</span>
            <h2>
              <code
                style={{
                  fontSize: "0.7em",
                  background: "var(--raised)",
                  padding: "2px 8px",
                  borderRadius: "var(--r-sm)",
                }}
              >
                prefetch.rs
              </code>{" "}
              server prefetch
            </h2>
            <p>
              Put a <code>prefetch.rs</code> next to a page and the server streams its
              data into the React Query cache before the component mounts. Delete
              it and the page just fetches on mount instead — the component
              can&apos;t tell the difference.
            </p>
          </div>
          <div className="code">
            <div className="code-head">
              <span className="dot" />
              <span>app/prefetch.rs</span>
            </div>
            <pre>
              <code>{PREFETCH_RS}</code>
            </pre>
          </div>
        </div>
      </section>

      <section className="section">
        <div className="shell">
          <div className="section-head">
            <span className="eyebrow">Live from production</span>
            <h2>Cold starts, measured continuously.</h2>
            <p>
              Every two hours a burst of requests hits real nextrs apps in
              production — pages and API routes. Each response says whether it
              paid a cold start. No lab, no cherry-picking; this table is the
              running total. <a href="/docs/case-study-hhh">How we measure →</a>
            </p>
          </div>
          <LiveColdstarts />
        </div>
      </section>

      <section className="section">
        <div className="shell">
          <div className="cta-band">
            <span className="eyebrow">Beta · v0.4</span>
            <h2>Build the next thing in Rust.</h2>
            <p>
              Scaffold an app, write React, deploy a single function. The docs walk
              you from zero to a deployed app on Vercel.
            </p>
            <div className="hero-cta">
              <a className="btn btn-primary" href="/docs/getting-started">
                Get started →
              </a>
              <a
                className="btn btn-ghost"
                href="https://github.com/drewhirschi/nextrs"
                rel="noopener"
              >
                GitHub
              </a>
            </div>
          </div>
        </div>
      </section>
    </>
  );
}
