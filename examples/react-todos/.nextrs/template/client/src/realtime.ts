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

export interface RealtimeSubscriptionOptions<T> {
  topic: string;
  origin?: string;
  onChange: (frame: RealtimeChangeFrame<T>) => void;
  onReady?: (frame: RealtimeReadyFrame) => void;
  onResync?: (frame: RealtimeResyncFrame) => void;
  onStatus?: (status: RealtimeStatus) => void;
  onError?: (error: Error) => void;
  socketFactory?: (url: string) => WebSocket;
  minReconnectDelayMs?: number;
  maxReconnectDelayMs?: number;
}

export interface RealtimeSubscription { close(): void; }

export function realtimeUrl(topic: string, origin?: string): string {
  const base = new URL(origin ?? window.location.origin);
  if (base.protocol === "http:") base.protocol = "ws:";
  if (base.protocol === "https:") base.protocol = "wss:";
  if (base.protocol !== "ws:" && base.protocol !== "wss:") throw new Error(`realtime origin must use http(s) or ws(s), received ${base.protocol}`);
  base.pathname = `/__nx/realtime/${encodeURIComponent(topic)}`;
  base.search = "";
  base.hash = "";
  return base.toString();
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
  const requestResync = (topic: string, sequence: number) => options.onResync?.({ type: "resync", topic, sequence });
  const connect = () => {
    if (stopped) return;
    report(attempt === 0 ? "connecting" : "reconnecting");
    try { socket = socketFactory(realtimeUrl(options.topic, options.origin)); }
    catch (error) { options.onError?.(asError(error)); reconnect(); return; }
    socket.onmessage = (message) => {
      const frame = parseRealtimeFrame<T>(message.data);
      if (!frame || frame.topic !== options.topic) return;
      if (frame.type === "ready") { attempt = 0; lastSequence = frame.sequence; report("live"); options.onReady?.(frame); return; }
      if (frame.type === "resync") { lastSequence = frame.sequence; requestResync(frame.topic, frame.sequence); return; }
      if (lastSequence !== undefined && frame.sequence !== lastSequence + 1) { lastSequence = frame.sequence; requestResync(frame.topic, frame.sequence); return; }
      lastSequence = frame.sequence;
      options.onChange(frame);
    };
    socket.onerror = () => options.onError?.(new Error("realtime WebSocket error"));
    socket.onclose = reconnect;
  };
  connect();
  return { close() { stopped = true; if (timer !== undefined) clearTimeout(timer); socket?.close(1000, "subscription closed"); report("stopped"); } };
}

function parseRealtimeFrame<T>(input: unknown): RealtimeFrame<T> | undefined {
  if (typeof input !== "string") return undefined;
  try {
    const value: unknown = JSON.parse(input);
    if (!value || typeof value !== "object") return undefined;
    const frame = value as Record<string, unknown>;
    if (typeof frame.type !== "string" || typeof frame.topic !== "string" || typeof frame.sequence !== "number") return undefined;
    if (frame.type === "ready" || frame.type === "resync") return value as RealtimeFrame<T>;
    if (frame.type === "change" && (frame.operation === "upsert" || frame.operation === "delete" || frame.operation === "invalidate")) return value as RealtimeFrame<T>;
    return undefined;
  } catch { return undefined; }
}

function asError(error: unknown): Error { return error instanceof Error ? error : new Error(String(error)); }
