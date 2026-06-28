// Framework plumbing used by the entry wrappers nextrs generates for
// page.tsx routes. Not re-exported from index.ts — pages never import this.
import type { QueryClient } from "@tanstack/react-query";

interface SeedEntry {
  key: unknown[];
  data: unknown;
}

/// Read the seeds the server streamed into the page (see props.rs /
/// nextrs::QuerySeed). Absent tag (no props.rs) → no seeds.
export function readSeeds(): SeedEntry[] {
  const tag = document.getElementById("__nx_seeds__");
  if (!tag?.textContent) return [];
  try {
    return JSON.parse(tag.textContent) as SeedEntry[];
  } catch {
    return [];
  }
}

/// Load server seeds into the React Query cache before mount.
///
/// The server ships the bare handler body; orval's fetch client caches a
/// `{ data, status, headers }` envelope, so the wrapping happens here — the
/// one file that knows which client generator is in use.
export function seedQueryClient(qc: QueryClient): void {
  for (const entry of readSeeds()) {
    qc.setQueryData(entry.key, {
      data: entry.data,
      status: 200,
      headers: new Headers(),
    });
  }
}
