// Materialize the ignored generated-client package from tracked framework wiring.
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const nextrsDir = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(nextrsDir, "..");
const templateDir = join(nextrsDir, "template", "client");
const clientDir = join(nextrsDir, "client");
const rootPackage = JSON.parse(
  await readFile(join(rootDir, "package.json"), "utf8"),
);
const dependencies = {
  ...rootPackage.dependencies,
  ...rootPackage.devDependencies,
  ...rootPackage.optionalDependencies,
};
const clientEntry = Object.entries(dependencies).find(
  ([, value]) =>
    typeof value === "string" &&
    value
      .replace(/^file:/, "")
      .replace(/^\.\//, "")
      .replace(/\/$/, "") === ".nextrs/client",
);
if (!clientEntry) {
  throw new Error(
    "package.json must depend on the generated client via file:./.nextrs/client",
  );
}
const [clientName] = clientEntry;

async function writeIfChanged(path, contents) {
  let current;
  try {
    current = await readFile(path, "utf8");
  } catch {}
  if (current === contents) return;
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, contents);
}

async function materialize(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const source = join(dir, entry.name);
    if (entry.isDirectory()) {
      await materialize(source);
      continue;
    }
    const rel = relative(templateDir, source);
    let contents = await readFile(source, "utf8");
    if (rel === "package.json") {
      const manifest = JSON.parse(contents);
      manifest.name = clientName;
      contents = `${JSON.stringify(manifest, null, 2)}\n`;
    }
    await writeIfChanged(join(clientDir, rel), contents);
  }
}

await materialize(templateDir);
