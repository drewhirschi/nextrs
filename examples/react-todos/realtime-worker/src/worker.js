import { DurableObject } from "cloudflare:workers";

const PREFIX = "/__nx/realtime/";
const MAX_EVENT_BYTES = 64 * 1024;

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (!url.pathname.startsWith(PREFIX)) return new Response("Not found", { status: 404 });
    const topic = decodeURIComponent(url.pathname.slice(PREFIX.length));
    if (!topic || topic.includes("/")) return new Response("Invalid topic", { status: 400 });

    if (request.headers.get("upgrade")?.toLowerCase() === "websocket") {
      const authorized = await authorizeViewer(request, topic, env);
      if (!authorized) return new Response("Forbidden", { status: 403 });
    } else if (request.method === "POST") {
      if (!hasPublishSecret(request, env)) return new Response("Unauthorized", { status: 401 });
    } else {
      return new Response("Expected a WebSocket upgrade or POST", { status: 426 });
    }

    const room = env.REALTIME_ROOMS.getByName(topic);
    return room.fetch(request);
  },
};

/** One ordered coordinator per topic (normally one household resource). */
export class RealtimeRoom extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.ctx = ctx;
    this.sql = ctx.storage.sql;
    this.sql.exec(`
      CREATE TABLE IF NOT EXISTS realtime_state (
        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
        sequence INTEGER NOT NULL
      );
      INSERT OR IGNORE INTO realtime_state (singleton, sequence) VALUES (1, 0);
    `);
  }

  async fetch(request) {
    const url = new URL(request.url);
    const topic = decodeURIComponent(url.pathname.slice(PREFIX.length));
    if (request.headers.get("upgrade")?.toLowerCase() === "websocket") {
      const [client, server] = Object.values(new WebSocketPair());
      this.ctx.acceptWebSocket(server);
      server.serializeAttachment({ topic });
      server.send(JSON.stringify({ type: "ready", topic, sequence: this.sequence() }));
      return new Response(null, { status: 101, webSocket: client });
    }

    const raw = await request.text();
    if (new TextEncoder().encode(raw).byteLength > MAX_EVENT_BYTES) {
      return new Response("Event too large", { status: 413 });
    }
    const change = validChange(raw);
    if (!change) return new Response("Invalid change", { status: 400 });
    const sequence = this.nextSequence();
    const frame = JSON.stringify({ type: "change", topic, sequence, ...change });
    for (const socket of this.ctx.getWebSockets()) {
      try { socket.send(frame); } catch { socket.close(1011, "broadcast failed"); }
    }
    return Response.json({ sequence });
  }

  webSocketMessage(socket) {
    socket.close(1008, "subscriptions are receive-only");
  }

  webSocketClose() {}

  webSocketError() {}

  sequence() {
    return Number(this.sql.exec("SELECT sequence FROM realtime_state WHERE singleton = 1").one().sequence);
  }

  nextSequence() {
    return Number(this.sql.exec("UPDATE realtime_state SET sequence = sequence + 1 WHERE singleton = 1 RETURNING sequence").one().sequence);
  }
}

async function authorizeViewer(request, topic, env) {
  const endpoint = new URL("/api/realtime/authorize", env.APP_URL);
  endpoint.searchParams.set("topic", topic);
  const response = await fetch(endpoint, {
    headers: {
      cookie: request.headers.get("cookie") ?? "",
      "x-nextrs-realtime-origin": new URL(request.url).origin,
    },
  });
  return response.status === 204;
}

function hasPublishSecret(request, env) {
  const expected = env.REALTIME_PUBLISH_SECRET;
  const authorization = request.headers.get("authorization") ?? "";
  return typeof expected === "string" && expected.length >= 16 && authorization === `Bearer ${expected}`;
}

function validChange(raw) {
  try {
    const value = JSON.parse(raw);
    if (!value || typeof value !== "object") return null;
    if (value.operation === "invalidate") return { operation: "invalidate" };
    if (value.operation === "delete" && typeof value.key === "string") {
      return { operation: "delete", key: value.key };
    }
    if (value.operation === "upsert" && typeof value.key === "string" && value.value && typeof value.value === "object") {
      return { operation: "upsert", key: value.key, value: value.value };
    }
    return null;
  } catch {
    return null;
  }
}
