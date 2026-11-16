// nextrs::realtime client: subscribe to a topic's ordered change frames.
//
// Each (re)connect first asks *your* app for a ticket — a short-lived signed
// URL your route only hands out after deciding the user may watch the topic —
// then opens the WebSocket it names: the in-process hub locally, the
// Cloudflare relay in production. Frames carry a per-topic sequence; on a gap
// or a `resync` the subscriber calls `onResync` so you refetch the snapshot.

export type RealtimeOperation = "upsert" | "delete" | "invalidate";

export interface RealtimeReadyFrame {
  type: "ready";
  topic: string;
  sequence: number;
}

export interface RealtimeChangeFrame<T> {
  type: "change";
  topic: string;
  sequence: number;
  operation: RealtimeOperation;
  key?: string;
  value?: T;
}

export interface RealtimeResyncFrame {
  type: "resync";
  topic: string;
  sequence: number;
}

export type RealtimeFrame<T> = RealtimeReadyFrame | RealtimeChangeFrame<T> | RealtimeResyncFrame;
export type RealtimeStatus = "connecting" | "live" | "reconnecting" | "stopped";

/** What the app's ticket route returns (`nextrs::realtime::Ticket`). */
export interface RealtimeTicket {
  topic: string;
  url: string;
  expires_at: number;
}

export interface RealtimeSubscriptionOptions<T> {
  topic: string;
  /** Fetch a fresh ticket from your app (called before every connect). */
  ticket: () => Promise<RealtimeTicket>;
  onChange: (frame: RealtimeChangeFrame<T>) => void;
  /** Attached: reconcile with the source-of-truth snapshot now. */
  onReady?: (frame: RealtimeReadyFrame) => void;
  /** Changes were missed: refetch the snapshot. */
  onResync?: (frame: RealtimeResyncFrame) => void;
  onStatus?: (status: RealtimeStatus) => void;
  onError?: (error: Error) => void;
  socketFactory?: (url: string) => WebSocket;
  minReconnectDelayMs?: number;
  maxReconnectDelayMs?: number;
}

export interface RealtimeSubscription {
  close(): void;
}

/** Resolve a ticket URL (origin-relative for the in-process hub) to ws(s). */
export function realtimeSocketUrl(ticketUrl: string, origin?: string): string {
  const url = new URL(ticketUrl, origin ?? window.location.origin);
  if (url.protocol === "http:") url.protocol = "ws:";
  if (url.protocol === "https:") url.protocol = "wss:";
  if (url.protocol !== "ws:" && url.protocol !== "wss:") {
    throw new Error(`realtime URL must be http(s) or ws(s), received ${url.protocol}`);
  }
  return url.toString();
}

export function subscribeRealtime<T>(options: RealtimeSubscriptionOptions<T>): RealtimeSubscription {
  const socketFactory = options.socketFactory ?? ((url: string) => new WebSocket(url));
  const minDelay = Math.max(25, options.minReconnectDelayMs ?? 250);
  const maxDelay = Math.max(minDelay, options.maxReconnectDelayMs ?? 5_000);
  let socket: WebSocket | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;
  let attempt = 0;
  let lastSequence: number | undefined;
  const report = (status: RealtimeStatus) => options.onStatus?.(status);
  const reconnect = () => {
    if (stopped) return;
    report("reconnecting");
    timer = setTimeout(connect, Math.min(maxDelay, minDelay * 2 ** attempt++));
  };
  const resync = (topic: string, sequence: number) => options.onResync?.({ type: "resync", topic, sequence });
  const connect = async () => {
    if (stopped) return;
    report(attempt === 0 ? "connecting" : "reconnecting");
    try {
      const ticket = await options.ticket();
      if (stopped) return;
      socket = socketFactory(realtimeSocketUrl(ticket.url));
    } catch (error) {
      options.onError?.(asError(error));
      reconnect();
      return;
    }
    socket.onmessage = (message) => {
      const frame = parseRealtimeFrame<T>(message.data);
      if (!frame || frame.topic !== options.topic) return;
      if (frame.type === "ready") {
        attempt = 0;
        // A reconnect may have missed changes: the ready frame is the cue to
        // reconcile, so treat a reconnect's ready as a resync.
        const reconnected = lastSequence !== undefined && frame.sequence !== lastSequence;
        lastSequence = frame.sequence;
        report("live");
        options.onReady?.(frame);
        if (reconnected) resync(frame.topic, frame.sequence);
        return;
      }
      if (frame.type === "resync") {
        lastSequence = frame.sequence;
        resync(frame.topic, frame.sequence);
        return;
      }
      if (lastSequence !== undefined && frame.sequence !== lastSequence + 1) {
        lastSequence = frame.sequence;
        resync(frame.topic, frame.sequence);
        return;
      }
      lastSequence = frame.sequence;
      options.onChange(frame);
    };
    socket.onerror = () => options.onError?.(new Error("realtime WebSocket error"));
    socket.onclose = reconnect;
  };
  void connect();
  return {
    close() {
      stopped = true;
      if (timer !== undefined) clearTimeout(timer);
      if (socket) {
        socket.onclose = null;
        socket.close(1000, "subscription closed");
      }
      report("stopped");
    },
  };
}

function parseRealtimeFrame<T>(input: unknown): RealtimeFrame<T> | undefined {
  if (typeof input !== "string") return undefined;
  try {
    const value: unknown = JSON.parse(input);
    if (!value || typeof value !== "object") return undefined;
    const frame = value as Record<string, unknown>;
    if (typeof frame.type !== "string" || typeof frame.topic !== "string" || typeof frame.sequence !== "number") {
      return undefined;
    }
    if (frame.type === "ready" || frame.type === "resync") return value as RealtimeFrame<T>;
    if (
      frame.type === "change" &&
      (frame.operation === "upsert" || frame.operation === "delete" || frame.operation === "invalidate")
    ) {
      return value as RealtimeFrame<T>;
    }
    return undefined;
  } catch {
    return undefined;
  }
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
