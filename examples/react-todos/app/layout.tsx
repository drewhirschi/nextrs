// Root React layout: with the app shell, this chrome stays MOUNTED across
// navigations — clicking the plain <a> links below soft-navigates (TanStack
// Router swaps only the page leaf; no document load, the React Query cache
// survives). React 19 hoists <title>/<meta> into <head>.
import type { ReactNode } from "react";

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <main>
      <title>nextrs · react-todos</title>
      <link rel="icon" href="/favicon.svg" type="image/svg+xml" />
      <link rel="icon" href="/favicon.ico" sizes="32x32" />
      <link rel="apple-touch-icon" href="/apple-touch-icon.png" />
      <nav className="topnav">
        {/* Same wordmark as the docs site (nextrs-docs.vercel.app). */}
        <a href="/" className="wordmark">
          next<b>rs</b>
        </a>
        <span className="nav-tag">react-todos</span>
        <span className="muted"> · plain anchors, soft navigation</span>
        {/* Unseeded route (no prefetch.rs): hover preloads its chunk only. */}
        <a className="muted" href="/about">
          about
        </a>
      </nav>
      {children}
    </main>
  );
}
