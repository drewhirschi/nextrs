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

const COMPARISONS: {
  label: string;
  detail: string;
  rust: string;
  next?: string;
}[] = [
  { label: "This docs site", detail: "the site you\u2019re reading right now", rust: "nextrs-docs" },
  { label: "A todo app", detail: "small \u00b7 typed API + React Query", rust: "react-todos", next: "nextjs-todos" },
  { label: "A booking app", detail: "medium \u00b7 auth, Postgres, admin management", rust: "hhh-rs", next: "hhh-nextjs" },
];

function pick(apps: AppStats[], app: string, target: string) {
  return apps.find((a) => a.app === app && a.target === target);
}

function cell(v: number | null | undefined) {
  return v == null ? "\u2014" : `${v} ms`;
}

function LiveColdstarts() {
  const { data, isLoading, isError } = useGetColdstartStats({
    query: { refetchInterval: 60_000 },
  });
  const stats = data && data.status === 200 ? data.data : undefined;
  if (isLoading) return <p className="live-note">Loading live numbers\u2026</p>;
  if (isError || !stats) return <p className="live-note">Telemetry temporarily unavailable.</p>;
  if (stats.total_samples === 0)
    return <p className="live-note">Collecting first samples \u2014 check back shortly.</p>;

  const rows: {
    group: string;
    detail: string;
    framework: string;
    isRust: boolean;
    page: number | null | undefined;
    api: number | null | undefined;
    cold: number | null | undefined;
  }[] = [];
  for (const c of COMPARISONS) {
    for (const [framework, app] of [["nextrs", c.rust], ["Next.js", c.next]] as const) {
      if (!app) continue;
      const pageStats = pick(stats.apps, app, "page");
      const apiStats = pick(stats.apps, app, "api");
      rows.push({
        group: c.label,
        detail: c.detail,
        framework,
        isRust: framework === "nextrs",
        page: pageStats?.warm_p50_ms as number | null,
        api: apiStats?.warm_p50_ms as number | null,
        cold: (apiStats?.cold ? apiStats.cold_p50_ms : null) as number | null,
      });
    }
  }

  return (
    <div>
      <div style={{ overflowX: "auto" }}>
        <table className="live-table" style={{ width: "100%", borderCollapse: "collapse", fontSize: 14 }}>
          <thead>
            <tr>
              {["", "", "typical page load", "typical API route", "cold start"].map((h, i) => (
                <th key={i} style={{ textAlign: "left", padding: "6px 10px", opacity: 0.6, fontWeight: 600 }}>{h}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => {
              const firstOfGroup = i === 0 || rows[i - 1].group !== r.group;
              const groupSize = rows.filter((x) => x.group === r.group).length;
              return (
                <tr key={r.group + r.framework} style={{ borderTop: firstOfGroup ? "1px solid var(--line, #333)" : "none" }}>
                  {firstOfGroup ? (
                    <td rowSpan={groupSize} style={{ padding: "8px 10px", verticalAlign: "top" }}>
                      <b>{r.group}</b>
                      <div style={{ opacity: 0.55, fontSize: 12 }}>{r.detail}</div>
                    </td>
                  ) : null}
                  <td style={{ padding: "6px 10px", fontWeight: r.isRust ? 700 : 400 }}>{r.framework}</td>
                  <td style={{ padding: "6px 10px" }}>{cell(r.page)}</td>
                  <td style={{ padding: "6px 10px" }}>{cell(r.api)}</td>
                  <td style={{ padding: "6px 10px" }}>{cell(r.cold)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <p className="live-note" style={{ opacity: 0.6, fontSize: 13, marginTop: 10 }}>
        {stats.total_samples.toLocaleString()} samples and counting \u2014 real deployments
        on Vercel, probed at randomized times every ~2 hours, measured from the same
        place in the same minutes. Typical = median sequential request against a warm
        instance (what a user clicking around experiences). Cold start = median first
        request on a fresh instance, measured on API routes (Next.js pages can\u2019t
        self-report instance temperature). Aggregated by <code>/api/coldstarts</code>,
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
