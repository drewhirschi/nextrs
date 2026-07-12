import { list } from "@/lib/store";
import { Shell } from "./shell";

// Per-request so the seed reflects the current store — the analog of
// react-todos's prefetch.rs running on each request. The server reads the store
// and serializes the seed, but does NOT render the React tree to HTML: the
// list is rendered client-side (see shell.tsx, ssr:false), matching nextrs's
// CSR-shell architecture so the page-throughput comparison is server-to-server.
export const dynamic = "force-dynamic";

export default async function Page() {
  const initial = list(true);
  return <Shell initial={initial} />;
}
