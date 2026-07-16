import { NextResponse } from "next/server";

// Fleet cold-start telemetry endpoint — the Next.js counterpart of nextrs's
// built-in /__nx/health, emitting the SAME headers the fleet pinger
// classifies on (see nextrs repo metrics/fleet.json), so Rust and Next.js
// cold starts land in one comparable dataset.
//
// force-dynamic: a static route would be CDN-served and never measure a
// cold start. Vercel exposes no native cold/warm signal; module scope is
// per-instance, which is exactly the signal we need.
export const dynamic = "force-dynamic";

const BOOT = Date.now();
const BOOT_ID = crypto.randomUUID().replaceAll("-", "").slice(0, 16);
let served = false;

export async function GET() {
  const first = !served;
  served = true;
  const uptime = Date.now() - BOOT;
  const res = NextResponse.json({
    status: "ok",
    boot_id: BOOT_ID,
    uptime_ms: uptime,
    first_request: first,
  });
  res.headers.set("x-nextrs-cold", first ? "1" : "0");
  res.headers.set("x-nextrs-uptime-ms", String(uptime));
  res.headers.set("x-nextrs-boot-id", BOOT_ID);
  res.headers.set("cache-control", "no-store");
  return res;
}
