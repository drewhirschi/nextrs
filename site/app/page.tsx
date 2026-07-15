// The docs-site landing page — a React (page.tsx) route, mounted client-side
// into __nx_root__ by the bundle nextrs generates. This page dogfoods the
// React track; the docs pages under /docs stay server-rendered.
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

function LiveColdstarts() {
  const { data, isLoading, isError } = useGetColdstartStats({
    query: { refetchInterval: 60_000 },
  });
  const stats = data && data.status === 200 ? data.data : undefined;
  if (isLoading) return <p className="live-note">Loading live numbers…</p>;
  if (isError || !stats) return <p className="live-note">Telemetry temporarily unavailable.</p>;
  if (stats.total_samples === 0)
    return <p className="live-note">Collecting first samples — check back shortly.</p>;
  const rows = stats.apps.filter((a: AppStats) => a.cold + a.warm > 0);
  return (
    <div>
      <div className="stats-row" style={{ display: "flex", gap: 28, margin: "18px 0" }}>
        <Stat value={String(stats.total_samples)} label="samples collected" />
        <Stat value={String(rows.reduce((n: number, a: AppStats) => n + a.cold, 0))} label="cold starts observed" />
        <Stat value={String(rows.reduce((n: number, a: AppStats) => n + a.warm, 0))} label="warm responses" />
      </div>
      {rows.length > 0 && (
        <div style={{ overflowX: "auto" }}>
          <table className="live-table" style={{ width: "100%", borderCollapse: "collapse", fontSize: 14 }}>
            <thead>
              <tr>
                {["app", "target", "cold p50", "cold p95", "warm p50", "warm p95"].map((h) => (
                  <th key={h} style={{ textAlign: "left", padding: "6px 10px", opacity: 0.6 }}>{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((a: AppStats) => (
                <tr key={a.app + a.target} style={{ borderTop: "1px solid var(--line, #333)" }}>
                  <td style={{ padding: "6px 10px" }}>{a.app}</td>
                  <td style={{ padding: "6px 10px" }}>{a.target || "single-ping era"}</td>
                  <td style={{ padding: "6px 10px" }}>{fmtMs(a.cold_p50_ms as number | null)}</td>
                  <td style={{ padding: "6px 10px" }}>{fmtMs(a.cold_p95_ms as number | null)}</td>
                  <td style={{ padding: "6px 10px" }}>{fmtMs(a.warm_p50_ms as number | null)}</td>
                  <td style={{ padding: "6px 10px" }}>{fmtMs(a.warm_p95_ms as number | null)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <p className="live-note" style={{ opacity: 0.6, fontSize: 13, marginTop: 10 }}>
        Cold = the response that reported paying its instance&apos;s first request
        (<code>x-nextrs-cold</code>). Bursts of 20 concurrent requests per target,
        every ~2 h, stored in Turso, aggregated by <code>/api/coldstarts</code> —
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
├─ layout.html     `}
                <span className="c-dim">→ shell + nav</span>
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
