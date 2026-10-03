import { useParams as useRouterParams } from "@tanstack/react-router";
import { useEffect, useRef, useState } from "react";
import {
  subscribeRealtime,
  type RealtimeChangeFrame,
  type RealtimeStatus,
  type RealtimeTicket,
} from "./realtime";

// Matched route params for deeply nested components. The app shell's router
// keeps these values live across soft navigation.
export function useParams<
  T extends Record<string, string> = Record<string, string>,
>(): T {
  return useRouterParams({ strict: false }) as T;
}

// Generated TanStack Query hooks/options, plus nextrs URL-bound companions.
export * from "./generated/react-query";
export { HttpError } from "./http-error";

// Live updates for a topic (nextrs::realtime). `onChange` runs for every
// ordered change; `onResync` runs when frames were missed or the socket
// reconnected, and should refetch. The usual call invalidates the queries the
// topic covers on both:
//
//   useLiveTopic({ topic: "todos", ticket: ..., onChange: invalidate, onResync: invalidate });
export function useLiveTopic<T = unknown>(options: {
  topic: string;
  ticket: () => Promise<RealtimeTicket>;
  onChange: (frame: RealtimeChangeFrame<T>) => void;
  onResync?: () => void;
  enabled?: boolean;
}): RealtimeStatus {
  const [status, setStatus] = useState<RealtimeStatus>("connecting");
  // Latest callbacks without resubscribing on every render.
  const latest = useRef(options);
  latest.current = options;
  const enabled = options.enabled ?? true;
  useEffect(() => {
    if (!enabled) return;
    const subscription = subscribeRealtime<T>({
      topic: options.topic,
      ticket: () => latest.current.ticket(),
      onChange: (frame) => latest.current.onChange(frame),
      onResync: () => latest.current.onResync?.(),
      onStatus: setStatus,
    });
    return () => subscription.close();
  }, [options.topic, enabled]);
  return status;
}
