// Regenerate SUMMARY.md from coldstarts.ndjson. Run by the pinger workflow
// after each append; usable by hand:
//
//   node metrics/summarize.mjs <data-dir>
//
// where <data-dir> holds coldstarts.ndjson and receives SUMMARY.md.

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import path from "node:path";

const dir = process.argv[2];
if (!dir) {
  console.error("usage: node metrics/summarize.mjs <data-dir>");
  process.exit(2);
}
const dataPath = path.join(dir, "coldstarts.ndjson");
if (!existsSync(dataPath)) {
  console.error(`no ${dataPath} yet`);
  process.exit(0);
}

const records = readFileSync(dataPath, "utf8")
  .split("\n")
  .filter(Boolean)
  .map((l) => JSON.parse(l));

const pct = (arr, p) => {
  if (!arr.length) return null;
  const s = [...arr].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))];
};
const fmt = (v) => (v === null ? "—" : `${v} ms`);

const byApp = new Map();
for (const r of records) {
  if (!byApp.has(r.app)) byApp.set(r.app, []);
  byApp.get(r.app).push(r);
}

const lines = [];
lines.push("# nextrs fleet — cold-start telemetry");
lines.push("");
lines.push(
  `Real production apps pinged at randomized times (see \`metrics/ping.mjs\` on main). ` +
    `“cold” means the responding instance reported this ping as its first request ` +
    `(\`x-nextrs-cold: 1\` from \`/__nx/health\`) — the request that paid the cold start. ` +
    `Updated ${new Date().toISOString().slice(0, 16)}Z · ${records.length} samples total.`,
);
lines.push("");
lines.push("| App | Samples | Cold | Cold p50 / p95 | Warm p50 / p95 | Errors |");
lines.push("|---|---|---|---|---|---|");
for (const [app, rs] of [...byApp.entries()].sort()) {
  const ok = rs.filter((r) => r.ok && !r.error);
  const errors = rs.length - ok.length;
  const cold = ok.filter((r) => r.temp === "cold").map((r) => r.ms);
  const warm = ok.filter((r) => r.temp === "warm").map((r) => r.ms);
  const unknown = ok.filter((r) => r.temp === "unknown").map((r) => r.ms);
  const coldCell = cold.length
    ? `${cold.length} (${Math.round((100 * cold.length) / (cold.length + warm.length))}%)`
    : unknown.length
      ? "n/a (no telemetry yet)"
      : "0";
  const coldTimes = cold.length ? `${fmt(pct(cold, 50))} / ${fmt(pct(cold, 95))}` : "—";
  const warmSrc = warm.length ? warm : unknown; // legacy apps: undifferentiated timings
  const warmTimes = warmSrc.length
    ? `${fmt(pct(warmSrc, 50))} / ${fmt(pct(warmSrc, 95))}${warm.length ? "" : " (all samples)"}`
    : "—";
  lines.push(`| ${app} | ${rs.length} | ${coldCell} | ${coldTimes} | ${warmTimes} | ${errors} |`);
}
lines.push("");
lines.push("## Last 14 days, daily cold-start counts");
lines.push("");
const cutoff = Date.now() - 14 * 864e5;
const byDay = new Map();
for (const r of records) {
  const t = Date.parse(r.ts);
  if (t < cutoff || r.temp !== "cold") continue;
  const d = r.ts.slice(0, 10);
  byDay.set(d, (byDay.get(d) ?? 0) + 1);
}
if (byDay.size === 0) lines.push("_none recorded yet_");
for (const [d, n] of [...byDay.entries()].sort()) lines.push(`- ${d}: ${n}`);
lines.push("");

writeFileSync(path.join(dir, "SUMMARY.md"), lines.join("\n"));
console.log(`SUMMARY.md written (${records.length} samples, ${byApp.size} apps)`);
