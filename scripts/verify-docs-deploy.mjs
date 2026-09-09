// CI deploy receipt: verify the executing framework and commit, not merely HTTP 200.
import { readFileSync } from 'node:fs';
import { setTimeout } from 'node:timers/promises';
const expectedRevision = process.env.NEXTRS_BUILD_REVISION;
if (!/^[0-9a-f]{40}$/.test(expectedRevision ?? '')) throw new Error('NEXTRS_BUILD_REVISION must be a full git commit');
const manifest = readFileSync(new URL('../crates/nextrs/Cargo.toml', import.meta.url), 'utf8');
const expectedVersion = manifest.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const base = process.env.NEXTRS_DOCS_URL ?? 'https://nextrs.hirschi.dev';
let lastError;
for (let attempt = 0; attempt < 12; attempt++) {
  try {
    const response = await fetch(new URL('/__nx/version', base), { cache: 'no-store', signal: AbortSignal.timeout(10000) });
    if (!response.ok) throw new Error(`version endpoint returned ${response.status}`);
    const receipt = await response.json();
    if (receipt.framework !== expectedVersion || receipt.revision !== expectedRevision) {
      throw new Error(`expected ${expectedVersion} at ${expectedRevision}; received ${JSON.stringify(receipt)}`);
    }
    for (const path of ['/', '/docs/server-bundles']) {
      const page = await fetch(new URL(path, base), { signal: AbortSignal.timeout(10000) });
      if (!page.ok) throw new Error(`${path} returned ${page.status}`);
    }
    console.log(`Verified docs: nextrs ${expectedVersion}, commit ${expectedRevision}`);
    process.exit(0);
  } catch (error) { lastError = error; }
  if (attempt < 11) await setTimeout(5000);
}
throw lastError;
