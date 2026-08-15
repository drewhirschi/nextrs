// Framework plumbing used by the entry wrappers nextrs generates for
// page.tsx routes. This is intentionally outside both public package barrels.
import type { QueryClient } from "@tanstack/react-query";

interface SeedEntry {
  key: unknown[];
  data: unknown;
}

export function readSeeds(): SeedEntry[] {
  const tag = document.getElementById("__nx_seeds__");
  if (!tag?.textContent) return [];
  try {
    return JSON.parse(tag.textContent) as SeedEntry[];
  } catch {
    return [];
  }
}

export function seedQueryClient(qc: QueryClient): void {
  for (const entry of readSeeds()) {
    qc.setQueryData(entry.key, {
      data: entry.data,
      status: 200,
      headers: new Headers(),
    });
  }
}
