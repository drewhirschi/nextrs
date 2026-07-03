// Root React layout: with the app shell, this chrome stays MOUNTED across
// navigations — clicking the plain <a> links below soft-navigates (TanStack
// Router swaps only the page leaf; no document load, the React Query cache
// survives). React 19 hoists <title>/<meta> into <head>.
import type { ReactNode } from "react";

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <main>
      <title>nextrs · react-todos</title>
      <nav className="topnav">
        <a href="/">Todos</a>
        <span className="muted"> · plain anchors, soft navigation</span>
      </nav>
      {children}
    </main>
  );
}
