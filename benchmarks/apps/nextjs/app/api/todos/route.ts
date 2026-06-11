import { NextRequest, NextResponse } from "next/server";
import { add, list } from "@/lib/store";

// Cold-start instrumentation: Vercel exposes no cold/warm signal, so the
// function reports it. BOOT is set once per instance at module load; the first
// request on a fresh (cold) instance sees firstSeen === false.
const BOOT = Date.now();
let firstSeen = false;

function markColdStart(res: NextResponse) {
  const cold = !firstSeen;
  firstSeen = true;
  res.headers.set("x-cold", cold ? "1" : "0");
  res.headers.set("x-init-ms", String(Date.now() - BOOT));
  return res;
}

// GET /api/todos?status=open  — mirrors react-todos's route.rs get()
export async function GET(req: NextRequest) {
  const openOnly = req.nextUrl.searchParams.get("status") === "open";
  return markColdStart(NextResponse.json(list(openOnly)));
}

// POST /api/todos  — mirrors route.rs post()
export async function POST(req: NextRequest) {
  const body = (await req.json()) as { title: string };
  return NextResponse.json(add(body.title));
}
