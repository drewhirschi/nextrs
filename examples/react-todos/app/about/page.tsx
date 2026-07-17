// An UNSEEDED page: no prefetch.rs next to it, so the app shell emits no
// loader for this route. Hovering a link to it must NOT hit /__nx/prefetch
// (the endpoint only mirrors prefetch-backed routes) — the hover handler's
// router.preloadRoute only warms this page's JS chunk. This is the living
// reference for the mixed seeded/unseeded app shape; e2e/hover.mjs asserts
// the network behavior against it.
export default function About() {
  return (
    <section>
      <title>About · react-todos</title>
      <h1>About</h1>
      <p>
        react-todos is the worked example for <b>nextrs</b>: Next.js-style file
        routing and React pages on a Rust (Axum) server, deployable to Vercel
        as a single serverless function.
      </p>
      <p className="muted note">
        Unlike the todos pages, this page has no <code>prefetch.rs</code> —
        it renders without server-seeded data. Hovering its link preloads only
        the page&apos;s JS chunk; no data prefetch request is made.
      </p>
      <p>
        <a href="/">Back to todos</a>
      </p>
    </section>
  );
}
