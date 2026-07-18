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
  { label: "This docs site", detail: "the site you’re reading right now", rust: "nextrs-docs" },
  {
    label: "A todo app",
    detail: "small · typed API + React Query",
    rust: "react-todos",
    next: "nextjs-todos",
  },
  {
    label: "A booking app",
    detail: "medium · auth, Postgres, admin management",
    rust: "hhh-rs",
    next: "hhh-nextjs",
  },
];

type Metric = { p50: number | null; p90: number | null };

function pick(apps: AppStats[], app: string | undefined, target: string) {
  return app ? apps.find((a) => a.app === app && a.target === target) : undefined;
}

function MetricCell({
  m,
  vs,
  missingLabel = "—",
}: {
  m: Metric | null;
  vs?: Metric | null;
  missingLabel?: string;
}) {
  if (!m || m.p50 == null)
    return (
      <td style={{ padding: "6px 10px", whiteSpace: "nowrap", opacity: missingLabel === "—" ? 1 : 0.65 }}>
        {missingLabel}
      </td>
    );
  let diff: React.ReactNode = null;
  if (vs?.p50 != null && vs.p50 > 0) {
    const deltas = [
      { label: "p50", mine: m.p50, base: vs.p50 },
      ...(m.p90 != null && vs.p90 != null && vs.p90 > 0
        ? [{ label: "p90", mine: m.p90, base: vs.p90 }]
        : []),
    ];
    diff = (
      <span style={{ display: "inline-flex", gap: 8, marginLeft: 8 }}>
        {deltas.map((d) => {
          const ratio = d.mine / d.base;
          const pctDiff = Math.round(Math.abs(ratio - 1) * 100);
          const direction = ratio < 1 ? "lower" : ratio > 1 ? "higher" : "equal";
          return (
            <span
              key={d.label}
              title={`${d.label}: ${d.mine} ms / ${d.base} ms = ${ratio.toFixed(3)}`}
              style={{
                color:
                  ratio < 1
                    ? "var(--ok, #22c55e)"
                    : ratio > 1
                      ? "var(--warn, #f59e0b)"
                      : "inherit",
                fontWeight: 600,
                fontSize: 12,
              }}
            >
              {d.label} {ratio.toFixed(2)}× · {pctDiff}% {direction}
            </span>
          );
        })}
      </span>
    );
  }
  return (
    <td style={{ padding: "6px 10px", whiteSpace: "nowrap" }}>
      {m.p50} / {m.p90 ?? "—"} ms{diff}
    </td>
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
      ? { p50: (a.warm_p50_ms ?? null) as number | null, p90: (a.warm_p90_ms ?? null) as number | null }
      : { p50: (a.cold_p50_ms ?? null) as number | null, p90: (a.cold_p90_ms ?? null) as number | null };
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

  const coldPageMissingLabel = (app: string | undefined) => {
    const a = pick(stats.apps, app, "page");
    return delivery(a) === "cdn" ? "N/A · CDN cached" : "not instrumented";
  };

  return (
    <div>
      <div style={{ overflowX: "auto" }}>
        <table className="live-table" style={{ width: "100%", borderCollapse: "collapse", fontSize: 14 }}>
          <thead>
            <tr>
              {["", "", "warm page load", "cold page load (burst)", "warm API route", "cold API route (burst)"].map((h, i) => (
                <th key={i} style={{ textAlign: "left", padding: "6px 10px", opacity: 0.6, fontWeight: 600 }}>
                  {h}
                  {i >= 2 ? <span style={{ fontWeight: 400, opacity: 0.7 }}> · p50/p90</span> : null}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {COMPARISONS.map((c) => {
              const hasPair = !!c.next;
              const rustRow = (
                <tr key={c.label + "rs"} style={{ borderTop: hasPair ? "none" : "1px solid var(--line, #333)" }}>
                  {!hasPair ? (
                    <td style={{ padding: "8px 10px", verticalAlign: "top" }}>
                      <b>{c.label}</b>
                      <div style={{ opacity: 0.55, fontSize: 12 }}>{c.detail}</div>
                    </td>
                  ) : null}
                  <td style={{ padding: "6px 10px", fontWeight: 700 }}>nextrs</td>
                  <MetricCell
                    m={metric(c.rust, "page", "warm")}
                    vs={comparable(c, "page") ? metric(c.next, "page", "warm") : null}
                  />
                  <MetricCell
                    m={metric(c.rust, "page", "cold")}
                    vs={comparable(c, "page") ? metric(c.next, "page", "cold") : null}
                  />
                  <MetricCell
                    m={metric(c.rust, "api", "warm")}
                    vs={comparable(c, "api") ? metric(c.next, "api", "warm") : null}
                  />
                  <MetricCell
                    m={metric(c.rust, "api", "cold")}
                    vs={comparable(c, "api") ? metric(c.next, "api", "cold") : null}
                  />
                </tr>
              );
              if (!hasPair) return rustRow;
              return (
                <React.Fragment key={c.label}>
                  <tr style={{ borderTop: "1px solid var(--line, #333)" }}>
                    <td rowSpan={2} style={{ padding: "8px 10px", verticalAlign: "top" }}>
                      <b>{c.label}</b>
                      <div style={{ opacity: 0.55, fontSize: 12 }}>{c.detail}</div>
                    </td>
                    <td style={{ padding: "6px 10px" }}>Next.js</td>
                    <MetricCell m={metric(c.next, "page", "warm")} />
                    <MetricCell m={metric(c.next, "page", "cold")} missingLabel={coldPageMissingLabel(c.next)} />
                    <MetricCell m={metric(c.next, "api", "warm")} />
                    <MetricCell m={metric(c.next, "api", "cold")} />
                  </tr>
                  {rustRow}
                </React.Fragment>
              );
            })}
          </tbody>
        </table>
      </div>
      <p className="live-note" style={{ opacity: 0.7, fontSize: 13, marginTop: 10, marginBottom: 0 }}>
        Comparison labels appear only when both observations used the same
        function region, that region matches the fleet configuration, and both
        requests used the same delivery path. Current checks:{" "}
        {COMPARISONS.filter((c) => c.next).map((c, i) => (
          <React.Fragment key={c.label + "status"}>
            {i ? " · " : ""}<b>{c.label}:</b>{" "}
            {comparisonStatus(c, "page")}; {comparisonStatus(c, "api")}
          </React.Fragment>
        ))}
      </p>
      <h3 style={{ marginTop: 28, marginBottom: 4 }}>How often does a request pay a cold start?</h3>
      <p className="live-note" style={{ opacity: 0.7, fontSize: 14, marginTop: 0, marginBottom: 8 }}>
        Each probe fires 20 concurrent API requests. This normalizes observed
        first requests by successful burst traffic; it does not infer how many
        requests any one instance served.
      </p>
      <div style={{ overflowX: "auto" }}>
        <table className="live-table" style={{ width: "100%", borderCollapse: "collapse", fontSize: 14 }}>
          <thead>
            <tr>
              {["", "", "successful burst requests", "fresh instances observed", "requests per observed cold"].map((h, i) => (
                <th key={i} style={{ textAlign: "left", padding: "6px 10px", opacity: 0.6, fontWeight: 600 }}>{h}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {COMPARISONS.filter((c) => c.next).map((c) => {
              const rows = [
                { framework: "Next.js", a: pick(stats.apps, c.next, "api") },
                { framework: "nextrs", a: pick(stats.apps, c.rust, "api") },
              ];
              const rpc = (a?: AppStats) =>
                a && a.burst_colds > 0 ? a.burst_requests / a.burst_colds : null;
              const jsRpc = rpc(rows[0].a);
              return rows.map((r, i) => {
                const mine = rpc(r.a);
                const coldRateRatio =
                  comparable(c, "api") &&
                  r.framework === "nextrs" && mine != null && jsRpc != null && jsRpc > 0
                    ? jsRpc / mine
                    : null;
                const pctChange =
                  coldRateRatio != null
                    ? Math.round(Math.abs(coldRateRatio - 1) * 100)
                    : null;
                return (
                  <tr key={c.label + r.framework} style={{ borderTop: i === 0 ? "1px solid var(--line, #333)" : "none" }}>
                    {i === 0 ? (
                      <td rowSpan={2} style={{ padding: "8px 10px", verticalAlign: "top" }}>
                        <b>{c.label}</b>
                      </td>
                    ) : null}
                    <td style={{ padding: "6px 10px", fontWeight: r.framework === "nextrs" ? 700 : 400 }}>{r.framework}</td>
                    <td style={{ padding: "6px 10px" }}>{r.a?.burst_requests ?? "—"}</td>
                    <td style={{ padding: "6px 10px" }}>{r.a?.burst_colds ?? "—"}</td>
                    <td style={{ padding: "6px 10px", whiteSpace: "nowrap" }}>
                      {mine == null ? "—" : `1 per ${mine.toFixed(1)}`}
                      {coldRateRatio != null && pctChange != null && pctChange >= 15 ? (
                        <span
                          style={{
                            color: coldRateRatio < 1 ? "var(--ok, #22c55e)" : "var(--warn, #f59e0b)",
                            fontWeight: 600,
                            fontSize: 12,
                            marginLeft: 8,
                          }}
                        >
                          {coldRateRatio.toFixed(2)}× cold rate · {pctChange}% {coldRateRatio < 1 ? "lower" : "higher"}
                        </span>
                      ) : null}
                    </td>
                  </tr>
                );
              });
            })}
          </tbody>
        </table>
      </div>
      <p className="live-note" style={{ opacity: 0.6, fontSize: 13, marginTop: 10 }}>
        {stats.total_samples.toLocaleString()} methodology-v{stats.telemetry_version} samples since the clean reset —
        real deployments on Vercel, probed at randomized times every ~2 hours,
        measured from the same runner in the same moments. Warm = sequential requests against a warm
        instance (what a user clicking around experiences). Cold = a request
        that started a fresh instance during a 20-request concurrent burst, so
        it includes real scale-out provisioning and burst contention. A cold-page value requires page-level
        instrumentation; a CDN-cached page has no function cold start.
        Aggregated by <code>/api/coldstarts</code>,
        the endpoint this page is calling right now.
      </p>
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
            <span className="eyebrow">Beta · v0.3</span>
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
