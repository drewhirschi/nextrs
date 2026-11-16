// An UNSEEDED page: no prefetch.rs next to it, so the app shell emits no
// loader for this route. Its JS chunk is warmed as soon as a link to it is on
// screen; showing intent to go there (hover, or a predicted one — pointer
// trajectory, Tab, scroll, touch) must NOT hit /__nx/prefetch, because the
// endpoint only mirrors prefetch-backed routes. This is the living reference
// for the mixed seeded/unseeded app shape; e2e/hover.mjs asserts the network
// behavior, the prediction and the preload stats against it.
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
        it renders without server-seeded data. Its JS chunk was preloaded the
        moment a link to it appeared on screen; no data prefetch is ever made.
      </p>
      <p className="muted note">
        The todo pages do have data, and it preloads when you show intent: the
        app shell predicts where the pointer, Tab key, or scroll is going. In
        the browser console, <code>__nx_preload_debug__()</code> draws the
        prediction and <code>__nx_preload_stats__()</code> shows how much it
        helped compared with plain hover.
      </p>
      <p>
        <a href="/">Back to todos</a>
      </p>
    </section>
  );
}
