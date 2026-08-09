import { useParams as useRouterParams } from "@tanstack/react-router";

export function useParams<T extends Record<string, string> = Record<string, string>>(): T {
  return useRouterParams({ strict: false }) as T;
}

export * from "./generated/react-query";
