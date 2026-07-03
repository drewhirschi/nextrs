// The client package surface. Pages import generated hooks/types plus the
// framework helpers below from "@react-todos/client".
import { useParams as useRouterParams } from "@tanstack/react-router";

// Matched route params ([seg] segments). Pages get them as a `params` prop;
// deep components can call this. Backed by the app shell's TanStack Router so
// the values stay LIVE across soft navigation — the server's __nx_params__
// tag is only the boot-time snapshot and goes stale after a client-side nav.
export function useParams<T extends Record<string, string> = Record<string, string>>(): T {
  return useRouterParams({ strict: false }) as T;
}

// Everything orval generates — React Query hooks for components, plus plain
// typed clients (getX/updateX functions and URL builders) for event handlers,
// scripts, and tests. `npm run gen` keeps ./generated/index.ts current.
export * from "./generated";
