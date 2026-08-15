import { useParams as useRouterParams } from "@tanstack/react-router";

// Matched route params for deeply nested components. The app shell's router
// keeps these values live across soft navigation.
export function useParams<
  T extends Record<string, string> = Record<string, string>,
>(): T {
  return useRouterParams({ strict: false }) as T;
}

// Generated TanStack Query hooks/options, plus nextrs URL-bound companions.
export * from "./generated/react-query";
